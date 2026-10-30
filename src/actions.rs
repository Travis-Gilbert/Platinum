//! Platinum's commands. The window binds their shortcuts (`main.rs`); the
//! browser view handles them, from a shortcut, the menu bar, the toolbar or
//! the navigator.

use gpui_kit::{Action, actions};

use crate::places::Place;
use crate::theme::Appearance;

actions!(
    frame,
    [
        ImportBrowserData,
        ShowProfiles,
        ShowBookmarks,
        ShowHistory,
        BookmarkPage,
        FillPassword,
        FillForm,
        SavePassword,
        NewTab,
        CloseTab,
        NextTab,
        PreviousTab,
        Reload,
        GoBack,
        GoForward,
        FocusAddressBar,
        OpenPalette,
        ToggleCompact,
        ToggleSidebar,
        TogglePlaces,
        LeavePlace,
        PinTab,
        ShowCanvas,
        ToggleDictation,
        ReadSelection,
        StopSpeaking,
        PauseSpeaking,
        ResumeSpeaking,
        VoiceSettings,
        ToggleNotes,
        NewCanvasNote,
        ArrangeCanvas,
        OverviewCanvas,
        ShowVisitTrail,
        ToggleAsk,
        ShowDownloads,
        ShowConsole,
        SharePage,
        Quit,
    ]
);

/// ⌘1 to ⌘8 pick that tab; ⌘9 picks the last one.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = frame, no_json)]
pub struct ActivateTab {
    pub index: usize,
}

/// Light, Dark or Match System.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = frame, no_json)]
pub struct SetAppearance {
    pub appearance: Appearance,
}

/// Show a place over the tabs or the workspace (the places bump, `places.rs`).
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = frame, no_json)]
pub struct OpenPlace {
    pub place: Place,
}

#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = frame, no_json)]
pub struct ChooseVoice {
    pub id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = frame, no_json)]
pub struct SpeechRate {
    pub words_per_minute: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasOperation {
    Open,
    Pin,
    Duplicate,
    Connect,
    Provenance,
    Remove,
    Group,
    Paste,
    ShowAll,
}
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = frame, no_json)]
pub struct CanvasAction {
    pub operation: CanvasOperation,
}
