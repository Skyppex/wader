//! LSP protocol models: plain data plus the trait impls that put them on
//! the wire. Nothing in here knows about Rill.
//!
//! Only the parts of the spec wader uses are modelled. Unknown fields in
//! incoming messages are ignored, and optional outgoing fields are left out
//! when unset.

/// A fieldless enum that the spec sends as a number.
macro_rules! int_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $($(#[$vmeta:meta])* $variant:ident = $value:literal),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant = $value,)*
        }

        impl $name {
            pub fn code(self) -> i32 {
                self as i32
            }

            pub fn from_code(code: i64) -> Option<$name> {
                match code {
                    $($value => Some($name::$variant),)*
                    _ => None,
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_i32(self.code())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<$name, D::Error> {
                let code = i64::deserialize(d)?;
                $name::from_code(code).ok_or_else(|| {
                    serde::de::Error::custom(format!(
                        concat!("unknown ", stringify!($name), " {}"),
                        code
                    ))
                })
            }
        }
    };
}

mod base;
mod completion;
mod diagnostic;
mod files;
mod hover;
mod lifecycle;
pub mod methods;
mod navigation;
mod rename;
mod signature;
mod sync;
mod window;

pub use base::*;
pub use completion::*;
pub use diagnostic::*;
pub use files::*;
pub use hover::*;
pub use lifecycle::*;
pub use methods::{Notification, Request, notification, request};
pub use navigation::*;
pub use rename::*;
pub use signature::*;
pub use sync::*;
pub use window::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn int_enums_are_numbers() {
        assert_eq!(
            serde_json::to_value(DiagnosticSeverity::Warning).unwrap(),
            json!(2)
        );
        assert_eq!(
            serde_json::from_value::<CompletionItemKind>(json!(14)).unwrap(),
            CompletionItemKind::Keyword
        );
        assert!(serde_json::from_value::<TextDocumentSyncKind>(json!(9)).is_err());
    }

    #[test]
    fn camel_case_and_skipped_nones() {
        let caps = ServerCapabilities {
            hover_provider: Some(true),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(caps).unwrap(),
            json!({"hoverProvider": true})
        );
    }

    #[test]
    fn initialize_params_tolerate_missing_and_extra_fields() {
        let p: InitializeParams = serde_json::from_value(json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {
                "general": {"positionEncodings": ["utf-8", "utf-16"]},
                "textDocument": {
                    "completion": {"completionItem": {"snippetSupport": true}},
                    "rename": {"prepareSupport": true},
                    "somethingNew": {}
                }
            },
            "trace": "off"
        }))
        .unwrap();
        let general = p.capabilities.general.unwrap();
        assert_eq!(
            general.position_encodings.unwrap()[0],
            PositionEncodingKind::utf8()
        );
        let td = p.capabilities.text_document.unwrap();
        assert_eq!(td.rename.unwrap().prepare_support, Some(true));

        let p: InitializeParams = serde_json::from_value(json!({})).unwrap();
        assert!(p.capabilities.general.is_none());
    }

    #[test]
    fn untagged_results() {
        let r = PrepareRenameResult::RangeWithPlaceholder {
            range: Range::default(),
            placeholder: "x".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["placeholder"], "x");
        assert_eq!(serde_json::from_value::<PrepareRenameResult>(v).unwrap(), r);

        let label = ParameterLabel::Offsets([3, 5]);
        assert_eq!(serde_json::to_value(&label).unwrap(), json!([3, 5]));
    }
}
