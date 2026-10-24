use std::cell::Cell;
use std::io;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use dispatch2::MainThreadBound;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString, NSURL};
use objc2_ui_kit::UIPasteboard;
use winit_core::data_transfer::{
    DataTransfer, DataTransferId, DataTransferSend, SendData, TransferType, TypeHint, TypedData,
};
use winit_core::error::{NotSupportedError, TransferError};

/// A uniform type identifier used by [`UIPasteboard`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteboardType(Arc<str>);

impl PasteboardType {
    /// Create a pasteboard type from a uniform type identifier.
    pub fn new(identifier: &str) -> Self {
        Self(identifier.into())
    }

    /// Get the uniform type identifier.
    pub fn identifier(&self) -> &str {
        &self.0
    }

    fn from_hint(hint: TypeHint) -> Option<Self> {
        Some(Self::new(match hint {
            TypeHint::Plaintext => "public.utf8-plain-text",
            TypeHint::Html => "public.html",
            TypeHint::Rtf => "public.rtf",
            TypeHint::UriList => "public.url",
            TypeHint::Image { extension_hint: None | Some("png") } => "public.png",
            TypeHint::Image { extension_hint: Some("tiff") } => "public.tiff",
            TypeHint::Image { extension_hint: Some("jpeg" | "jpg") } => "public.jpeg",
            _ => return None,
        }))
    }

    fn from_dyn(type_: &dyn TransferType) -> Option<Self> {
        type_.cast_ref::<Self>().cloned().or_else(|| type_.hint().and_then(Self::from_hint))
    }
}

impl TransferType for PasteboardType {
    fn hint(&self) -> Option<TypeHint> {
        match self.identifier() {
            "public.utf8-plain-text" | "public.plain-text" | "public.utf16-plain-text" => {
                Some(TypeHint::Plaintext)
            },
            "public.html" => Some(TypeHint::Html),
            "public.rtf" => Some(TypeHint::Rtf),
            "public.url" | "public.file-url" => Some(TypeHint::UriList),
            "public.png" => Some(TypeHint::Image { extension_hint: Some("png") }),
            "public.tiff" => Some(TypeHint::Image { extension_hint: Some("tiff") }),
            "public.jpeg" => Some(TypeHint::Image { extension_hint: Some("jpeg") }),
            _ => None,
        }
    }

    fn matches(&self, other: &dyn TransferType) -> bool {
        if let Some(other) = other.cast_ref::<Self>() {
            self == other
        } else {
            self.hint().zip(other.hint()).is_some_and(|(a, b)| a.matches(&b))
        }
    }
}

/// A snapshot of the clipboard's identity and available types.
#[derive(Debug)]
pub struct Pasteboard {
    pasteboard: MainThreadBound<Retained<UIPasteboard>>,
    change_count: isize,
    types: Arc<[PasteboardType]>,
}

impl Clone for Pasteboard {
    fn clone(&self) -> Self {
        Self {
            pasteboard: self.pasteboard.get_on_main(|pb| {
                MainThreadBound::new(pb.clone(), MainThreadMarker::new().unwrap())
            }),
            change_count: self.change_count,
            types: self.types.clone(),
        }
    }
}

impl Pasteboard {
    fn available_types(pasteboard: &UIPasteboard) -> Arc<[PasteboardType]> {
        let mut types = Vec::new();
        if let Some(items) = pasteboard.pasteboardTypesForItemSet(None) {
            for item in &items {
                for type_ in &item {
                    let type_ = PasteboardType::new(&type_.to_string());
                    if !types.contains(&type_) {
                        types.push(type_);
                    }
                }
            }
        }
        types.into()
    }

    fn new(pasteboard: Retained<UIPasteboard>, mtm: MainThreadMarker) -> Self {
        // SAFETY: All pasteboard access is confined to the main thread. Metadata queries do not
        // read clipboard contents; string/URLs/data reads below may show the iOS paste prompt.
        let change_count = unsafe { pasteboard.changeCount() };
        let types = Self::available_types(&pasteboard);
        Self { pasteboard: MainThreadBound::new(pasteboard, mtm), change_count, types }
    }

    fn try_access<R: Send>(&self, f: impl FnOnce(&UIPasteboard) -> R + Send) -> io::Result<R> {
        self.pasteboard.get_on_main(|pb| {
            // SAFETY: Running on the main thread.
            if unsafe { pb.changeCount() } != self.change_count {
                return Err(io::Error::other("pasteboard contents changed"));
            }
            let result = f(pb);
            // SAFETY: Running on the main thread.
            if unsafe { pb.changeCount() } != self.change_count {
                return Err(io::Error::other("pasteboard contents changed"));
            }
            Ok(result)
        })
    }

    pub(crate) fn with_type(
        &self,
        type_: &dyn TransferType,
    ) -> Result<PasteboardValue, TransferError> {
        let type_ = self
            .types
            .iter()
            .find(|candidate| candidate.matches(type_))
            .cloned()
            .ok_or(TransferError::Failed)?;
        Ok(PasteboardValue { pasteboard: self.clone(), type_ })
    }
}

impl DataTransfer for Pasteboard {
    fn for_each_available_type<'this>(
        &'this self,
        func: &mut dyn FnMut(&'this dyn TransferType) -> ControlFlow<()>,
    ) {
        let _ = self.types.iter().map(|type_| type_ as &dyn TransferType).try_for_each(func);
    }
}

/// A lazy reader for one clipboard representation.
#[derive(Debug)]
pub struct PasteboardValue {
    pasteboard: Pasteboard,
    type_: PasteboardType,
}

impl TypedData for PasteboardValue {
    fn type_(&self) -> &dyn TransferType {
        &self.type_
    }

    fn try_read(&self) -> Option<Box<dyn io::BufRead>> {
        self.try_as_bytes().ok().map(|bytes| Box::new(io::Cursor::new(bytes)) as _)
    }

    fn try_as_bytes(&self) -> io::Result<Vec<u8>> {
        match self.type_.hint() {
            Some(TypeHint::Plaintext) => self.try_as_string().map(String::into_bytes),
            Some(TypeHint::UriList) => self.try_as_uris().map(|uris| {
                uris.into_iter().map(|uri| uri + "\r\n").collect::<String>().into_bytes()
            }),
            _ => self.pasteboard.try_access(|pb| {
                pb.dataForPasteboardType_inItemSet(
                    &NSString::from_str(self.type_.identifier()),
                    None,
                )
                .and_then(|items| items.firstObject().map(|data| data.to_vec()))
                .ok_or_else(|| io::ErrorKind::InvalidData.into())
            })?,
        }
    }

    fn try_as_string(&self) -> io::Result<String> {
        if self.type_.hint() == Some(TypeHint::Plaintext) {
            self.pasteboard.try_access(|pb| {
                // SAFETY: Running on the main thread. UIKit converts native text to NSString.
                unsafe { pb.strings() }
                    .and_then(|strings| strings.firstObject())
                    .map(|s| s.to_string())
                    .ok_or_else(|| io::ErrorKind::InvalidData.into())
            })?
        } else {
            String::from_utf8(self.try_as_bytes()?)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
    }

    fn try_as_uris(&self) -> io::Result<Vec<String>> {
        if self.type_.hint() != Some(TypeHint::UriList) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.pasteboard.try_access(|pb| {
            // SAFETY: Running on the main thread.
            let urls = unsafe { pb.URLs() }.ok_or(io::ErrorKind::InvalidData)?;
            urls.iter()
                .filter(|url| self.type_.identifier() != "public.file-url" || url.isFileURL())
                .map(|url| {
                    url.absoluteString()
                        .map(|s| s.to_string())
                        .ok_or_else(|| io::ErrorKind::InvalidData.into())
                })
                .collect()
        })?
    }
}

#[derive(Debug, Default)]
pub(crate) struct DataTransferState {
    clipboard: Cell<Option<(isize, DataTransferId)>>,
}

impl DataTransferState {
    fn clipboard_transfer_id(&self, change_count: isize) -> DataTransferId {
        // Counts downward to keep clipboard IDs disjoint from future drag IDs.
        static NEXT_ID: AtomicI64 = AtomicI64::new(-1);
        match self.clipboard.get() {
            Some((cached, id)) if cached == change_count => id,
            _ => {
                let id = DataTransferId::from_raw(NEXT_ID.fetch_sub(1, Ordering::Relaxed));
                self.clipboard.set(Some((change_count, id)));
                id
            },
        }
    }

    pub(crate) fn clipboard(&self, _mtm: MainThreadMarker) -> Option<DataTransferId> {
        let pb = UIPasteboard::generalPasteboard();
        if Pasteboard::available_types(&pb).is_empty() {
            return None;
        }
        Some(self.clipboard_transfer_id(unsafe { pb.changeCount() }))
    }

    pub(crate) fn by_id(
        &self,
        id: DataTransferId,
        mtm: MainThreadMarker,
    ) -> Result<Pasteboard, TransferError> {
        let pb = Pasteboard::new(UIPasteboard::generalPasteboard(), mtm);
        if self.clipboard.get() != Some((pb.change_count, id)) {
            return Err(TransferError::UnknownTransfer(id));
        }
        Ok(pb)
    }
}

pub(crate) fn set_clipboard(
    data: &dyn DataTransferSend,
    _mtm: MainThreadMarker,
) -> Result<(), TransferError> {
    let mut keys = Vec::new();
    let mut values = Vec::<Retained<AnyObject>>::new();
    let mut uri_items = Vec::new();
    for type_ in data.available_types() {
        let Some(pb_type) = PasteboardType::from_dyn(type_) else { continue };
        let Some(value) = data.data_for_type(type_) else { continue };
        let value = match value {
            SendData::String(value) => NSString::from_str(&value).into(),
            SendData::Bytes(value) => NSData::with_bytes(&value).into(),
            SendData::Uris(uris) => {
                if pb_type.hint() != Some(TypeHint::UriList) {
                    return Err(TransferError::Failed);
                }
                for uri in uris {
                    let url = NSURL::URLWithString(&NSString::from_str(&uri))
                        .ok_or(TransferError::Failed)?;
                    let key = NSString::from_str(pb_type.identifier());
                    let value: Retained<AnyObject> = url.into();
                    uri_items.push(NSDictionary::from_slices(&[&*key], &[&*value]));
                }
                continue;
            },
            _ => continue,
        };
        keys.push(NSString::from_str(pb_type.identifier()));
        values.push(value);
    }
    if !keys.is_empty() {
        let keys = keys.iter().map(|key| &**key).collect::<Vec<_>>();
        uri_items.insert(0, NSDictionary::from_retained_objects(&keys, &values));
    }
    if uri_items.is_empty() {
        return Err(NotSupportedError::new("no supported clipboard representations").into());
    }
    let items = NSArray::from_retained_slice(&uri_items);
    // SAFETY: Running on the main thread; values are NSString, NSData or NSURL as required by
    // UIKit.
    unsafe { UIPasteboard::generalPasteboard().setItems(&items) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_matching() {
        assert!(PasteboardType::new("public.plain-text").matches(&TypeHint::Plaintext));
        assert!(PasteboardType::new("public.file-url").matches(&TypeHint::UriList));
        assert!(
            PasteboardType::new("public.png").matches(&TypeHint::Image { extension_hint: None })
        );
        let custom = PasteboardType::new("org.example.custom");
        assert!(custom.matches(&custom));
        assert!(!custom.matches(&TypeHint::Plaintext));
        assert!(PasteboardType::from_hint(TypeHint::Audio { extension_hint: None }).is_none());
    }

    #[test]
    fn clipboard_ids_are_cached_and_invalidated() {
        let state = DataTransferState::default();
        assert_eq!(state.clipboard.get(), None);
        let id = state.clipboard_transfer_id(0);
        assert!(id.into_raw() < 0);
        assert_eq!(state.clipboard_transfer_id(0), id);
        assert_ne!(state.clipboard_transfer_id(1), id);
    }
}
