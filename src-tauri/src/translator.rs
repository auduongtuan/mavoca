//! On-device English to Vietnamese translation through Apple's Translation
//! framework. Like `dictionary`, it has no database or Tauri dependencies.
//!
//! Translation is free and offline, but the language pair must already be
//! downloaded in System Settings; the framework will not fetch it for a session
//! built without SwiftUI.

use translation::prelude::*;

const SOURCE_LANGUAGE: &str = "en";
const TARGET_LANGUAGE: &str = "vi";

const NOT_INSTALLED_HINT: &str = "Vietnamese isn't downloaded yet. Add it in System Settings → General → Language & Region → Translation Languages.";

/// Translates each text in order, keeping empty strings empty.
///
/// Blocks until done and must not run on the main thread: the framework
/// completes its work on the main queue, so calling this from the main thread
/// stalls until the bridge times out.
pub fn translate_to_vietnamese(texts: &[String]) -> Result<Vec<String>, String> {
    let session = TranslationSession::new(TranslationSessionConfiguration::new(
        SOURCE_LANGUAGE,
        TARGET_LANGUAGE,
    ))
    .map_err(describe_error)?;

    texts
        .iter()
        .map(|text| {
            if text.trim().is_empty() {
                return Ok(String::new());
            }
            session
                .translate(text)
                .map(|response| response.target_text().to_string())
                .map_err(describe_error)
        })
        .collect()
}

fn describe_error(error: TranslationError) -> String {
    match error {
        TranslationError::NotInstalled(_) => NOT_INSTALLED_HINT.into(),
        TranslationError::UnavailableOnThisMacOS(_) => {
            "Translation needs macOS 26 or later.".into()
        }
        other => format!("Translation failed: {other}"),
    }
}
