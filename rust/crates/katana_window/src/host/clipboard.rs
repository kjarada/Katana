//! The clipboard the window copies to and pastes from: text (the command line, a report) and an
//! image (SNAPSHOT CLIPBOARD, a view copied). The host's, never the process's (the plan's section
//! 4.7): in memory in a test and in a headless run - so the nine clipboard test files run on
//! ordinary threads, as Qt's offscreen clipboard is in memory - and the desktop's on the desktop.

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};

use crate::canvas::Image;

/// QClipboard, as the window uses it.
pub trait Clipboard {
    /// The text on the clipboard; None when it holds none.
    fn text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str) -> Status;
    /// The image on the clipboard; None when it holds none.
    fn image(&mut self) -> Option<Image>;
    fn set_image(&mut self, image: &Image) -> Status;
}

/// A clipboard of the host's own, in memory: a test's and a headless run's. Setting the text
/// leaves no image and setting the image no text, as a system clipboard holds one thing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryClipboard {
    text: Option<String>,
    image: Option<Image>,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clipboard for MemoryClipboard {
    fn text(&mut self) -> Option<String> {
        self.text.clone()
    }

    fn set_text(&mut self, text: &str) -> Status {
        self.text = Some(String::from(text));
        self.image = None;
        Ok(())
    }

    fn image(&mut self) -> Option<Image> {
        self.image.clone()
    }

    fn set_image(&mut self, image: &Image) -> Status {
        self.image = Some(image.clone());
        self.text = None;
        Ok(())
    }
}

/// The desktop's clipboard, through arboard. Made only by the desktop program: on X11 it connects
/// to the display, which no test and no headless run may do (WSLg would answer from the owner's
/// desktop). A clipboard that cannot be opened answers nothing and refuses to be set, saying why.
pub struct DesktopClipboard {
    system: std::result::Result<arboard::Clipboard, String>,
}

impl DesktopClipboard {
    pub fn new() -> Self {
        Self {
            system: arboard::Clipboard::new().map_err(|error| format!("{error}")),
        }
    }

    fn refused(&self, what: &str, why: &str) -> katana_core::Error {
        make_error(
            ErrorCode::InvalidState,
            format!("the clipboard could not take the {what}"),
            why,
        )
    }
}

impl Default for DesktopClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for DesktopClipboard {
    fn text(&mut self) -> Option<String> {
        self.system.as_mut().ok()?.get_text().ok()
    }

    fn set_text(&mut self, text: &str) -> Status {
        let answer = match self.system.as_mut() {
            Ok(system) => system.set_text(text).map_err(|error| format!("{error}")),
            Err(why) => Err(why.clone()),
        };
        answer.map_err(|why| self.refused("text", &why))
    }

    fn image(&mut self) -> Option<Image> {
        let data = self.system.as_mut().ok()?.get_image().ok()?;
        let width = u32::try_from(data.width).ok()?;
        let height = u32::try_from(data.height).ok()?;
        Image::from_straight_rgba8(width, height, data.bytes.into_owned()).ok()
    }

    fn set_image(&mut self, image: &Image) -> Status {
        let data = arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Owned(image.to_straight_rgba8()),
        };
        let answer = match self.system.as_mut() {
            Ok(system) => system.set_image(data).map_err(|error| format!("{error}")),
            Err(why) => Err(why.clone()),
        };
        answer.map_err(|why| self.refused("image", &why))
    }
}

/// The text on `clipboard`, or the refusal a paste of nothing gives.
pub fn clipboard_text(clipboard: &mut dyn Clipboard) -> Result<String> {
    clipboard.text().ok_or_else(|| {
        make_error(
            ErrorCode::NotFound,
            "the clipboard holds no text",
            String::new(),
        )
    })
}
