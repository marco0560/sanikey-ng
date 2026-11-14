//! Typed, privacy-preserving events for the session logging infrastructure.
//!
//! This module is an initial building block. It does not enable logging or
//! connect the sink to application operations on its own.

mod context;
mod trace;

pub use trace::{
    TraceField, TraceLink, TraceOmission, TraceSpan, TraceValue, redact_secret_assignments,
    trace_current, trace_json_value, trace_optional_toml_value, trace_sensitive_text,
    trace_toml_table, trace_toml_value,
};

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs::{DirBuilder, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use rustix::fs::{FileType, Mode, OFlags, fstat, open, openat};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

fn timestamp_unix_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|time| u64::try_from(time.as_millis()).ok())
}

/// Diagnostic detail explicitly selected for one session.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Level {
    /// Fixed, non-private operation events.
    Events,
    /// Includes displayed text, including private and unclassified values.
    Diagnostic,
    /// Includes detailed function and data tracing when instrumented.
    Trace,
}

/// Kind of native picker whose contents the SaniKey GUI cannot observe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeDialog {
    /// Account-profile file picker.
    ProfileFile,
    /// USB target folder picker.
    UsbTargetFolder,
    /// Folder picker attached to a path field.
    PathFolder,
}

/// Maps native picker kinds to stable, non-sensitive log identifiers.
impl NativeDialog {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ProfileFile => "profile_file",
            Self::UsbTargetFolder => "usb_target_folder",
            Self::PathFolder => "path_folder",
        }
    }
}

/// Maps the selected detail level to its stable log identifier.
impl Level {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Diagnostic => "diagnostic",
            Self::Trace => "trace",
        }
    }
}

/// A static control name with non-sensitive numeric position in repeated UI.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DisplayControl {
    name: &'static str,
    index: Option<usize>,
    child_index: Option<usize>,
    leaf_index: Option<usize>,
    part: DisplayPart,
}

/// Part of a composite control producing visible text.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DisplayPart {
    /// Editable or computed value.
    Value,
    /// Associated label.
    Label,
    /// Action caption.
    Action,
}

impl DisplayControl {
    /// Identifies a single control using an application-defined name.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            index: None,
            child_index: None,
            leaf_index: None,
            part: DisplayPart::Value,
        }
    }

    /// Identifies a control in a repeated section without using user data.
    #[must_use]
    pub const fn indexed(name: &'static str, index: usize, child_index: Option<usize>) -> Self {
        Self {
            name,
            index: Some(index),
            child_index,
            leaf_index: None,
            part: DisplayPart::Value,
        }
    }

    /// Identifies a control in a three-level repeated section.
    #[must_use]
    pub const fn nested(
        name: &'static str,
        index: usize,
        child_index: usize,
        leaf_index: usize,
    ) -> Self {
        Self {
            name,
            index: Some(index),
            child_index: Some(child_index),
            leaf_index: Some(leaf_index),
            part: DisplayPart::Value,
        }
    }

    /// Selects a distinct text-bearing part of this control.
    #[must_use]
    pub const fn with_part(mut self, part: DisplayPart) -> Self {
        self.part = part;
        self
    }

    fn to_json(self) -> Value {
        json!({"name": self.name, "index": self.index, "child_index": self.child_index, "leaf_index": self.leaf_index, "part": self.part.as_str()})
    }
}

/// Maps displayed control parts to their log identifiers.
impl DisplayPart {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Label => "label",
            Self::Action => "action",
        }
    }
}

/// Returns the per-user log directory independently of the current directory.
///
/// # Errors
///
/// Returns an error when no suitable absolute user directory is configured.
pub fn default_log_directory() -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = absolute_environment_path("LOCALAPPDATA");
    #[cfg(target_os = "macos")]
    let base = absolute_environment_path("HOME").map(|home| home.join("Library/Logs"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = xdg_state_directory(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    );
    base.map(|directory| directory.join("sanikey").join("logs"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "directory locale per i log non disponibile",
            )
        })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn xdg_state_directory(state: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    state.filter(|path| path.is_absolute()).or_else(|| {
        home.filter(|path| path.is_absolute())
            .map(|home| home.join(".local/state"))
    })
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn absolute_environment_path(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Component producing a low-detail event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Component {
    /// Graphical application.
    Desktop,
    /// Command-line application.
    Cli,
    /// Shared domain services.
    Core,
}

impl Component {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Cli => "cli",
            Self::Core => "core",
        }
    }
}

/// Fixed vocabulary for events that must not contain user-provided text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    /// A session or operation started.
    Started,
    /// An operation reported a phase or progress.
    Progress,
    /// An operation ended successfully.
    Succeeded,
    /// An operation failed.
    Failed,
    /// An operation was cancelled.
    Cancelled,
    /// A session ended.
    Finished,
    /// An operation ended without reporting a terminal outcome.
    Unfinished,
    /// The frontend is closing while work remains active.
    ShutdownPending,
}

/// Non-sensitive category for a failed operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureCategory {
    /// Command-line syntax or usage failure.
    Usage,
    /// Invalid or unavailable configuration.
    Configuration,
    /// Privacy validation failure.
    Privacy,
    /// Removable-volume operation failure.
    Usb,
    /// Failure not classified more precisely.
    Other,
}

/// Coarse stage of a running operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressPhase {
    /// Checking prerequisites.
    Preparing,
    /// Processing inputs.
    Processing,
    /// Writing or validating outputs.
    Finalizing,
}

impl ProgressPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Processing => "processing",
            Self::Finalizing => "finalizing",
        }
    }
}

/// Maps failure categories to stable log identifiers.
impl FailureCategory {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Configuration => "configuration",
            Self::Privacy => "privacy",
            Self::Usb => "usb",
            Self::Other => "other",
        }
    }
}

/// Identifies an activity without including user-supplied arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activity {
    /// Lifetime of the logging session.
    Session,
    /// Command-line dispatch.
    Command,
    /// The CLI information command.
    CliInfo,
    /// The CLI single-person build command.
    CliBuildPatient,
    /// The CLI multi-person build command.
    CliBuildAll,
    /// The CLI help command.
    CliHelp,
    /// The CLI version command.
    CliVersion,
    /// Profile import.
    Profile,
    /// Preflight and duplicate review.
    Preflight,
    /// Patient build.
    Build,
    /// Observation import.
    Observations,
    /// AIFA review and decisions.
    Aifa,
    /// USB image preparation or copy.
    Usb,
    /// Another core operation not otherwise classified.
    OtherOperation,
    /// GUI profile path edited.
    GuiProfilePathEdited,
    /// GUI folder path edited.
    GuiFolderEdited,
    /// GUI folder chosen with the file dialog.
    GuiFolderChosen,
    /// GUI person selection changed.
    GuiPersonSelection,
    /// GUI review returned to selection editing.
    GuiReviewEdit,
    /// GUI USB confirmation changed.
    GuiUsbConfirmation,
    /// GUI AIFA candidate selection changed.
    GuiAifaCandidate,
    /// GUI profile dialog opened.
    GuiProfileDialogOpened,
    /// GUI AIFA query edited.
    GuiAifaQueryEdited,
    /// GUI panel expanded or collapsed.
    GuiPanelToggled,
    /// GUI logging level selection changed.
    GuiLogLevelSelected,
    /// GUI consent selection changed.
    GuiConsentChanged,
    /// GUI logging deactivation requested.
    GuiLoggingStop,
}

impl Activity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Command => "command",
            Self::CliInfo => "cli_info",
            Self::CliBuildPatient => "cli_build_patient",
            Self::CliBuildAll => "cli_build_all",
            Self::CliHelp => "cli_help",
            Self::CliVersion => "cli_version",
            Self::Profile => "profile",
            Self::Preflight => "preflight",
            Self::Build => "build",
            Self::Observations => "observations",
            Self::Aifa => "aifa",
            Self::Usb => "usb",
            Self::OtherOperation => "other_operation",
            Self::GuiProfilePathEdited => "gui_profile_path_edited",
            Self::GuiFolderEdited => "gui_folder_edited",
            Self::GuiFolderChosen => "gui_folder_chosen",
            Self::GuiPersonSelection => "gui_person_selection",
            Self::GuiReviewEdit => "gui_review_edit",
            Self::GuiUsbConfirmation => "gui_usb_confirmation",
            Self::GuiAifaCandidate => "gui_aifa_candidate",
            Self::GuiProfileDialogOpened => "gui_profile_dialog_opened",
            Self::GuiAifaQueryEdited => "gui_aifa_query_edited",
            Self::GuiPanelToggled => "gui_panel_toggled",
            Self::GuiLogLevelSelected => "gui_log_level_selected",
            Self::GuiConsentChanged => "gui_consent_changed",
            Self::GuiLoggingStop => "gui_logging_stop",
        }
    }
}

/// Maps event kinds to stable log identifiers.
impl EventKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Progress => "progress",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Finished => "finished",
            Self::Unfinished => "unfinished",
            Self::ShutdownPending => "shutdown_pending",
        }
    }
}

/// A minimal event containing only fixed identifiers and numeric measurements.
///
/// Free-form messages, paths and arguments have no representation in this
/// type, so they cannot accidentally enter the minimal event stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Event {
    /// Producing component.
    pub component: Component,
    /// Lifecycle state.
    pub kind: EventKind,
    /// Fixed activity identifier.
    pub activity: Activity,
    /// Failure category, when available.
    pub failure_category: Option<FailureCategory>,
    /// Session-local operation identifier, when available.
    pub operation_id: Option<u64>,
    /// Containing operation in this session, if one is scoped.
    pub parent_operation_id: Option<u64>,
    /// Static control identity and numeric indices for a GUI action.
    pub target: Option<DisplayControl>,
    /// Duration in milliseconds, when available.
    pub duration_ms: Option<u64>,
}

impl Event {
    /// Builds the structured representation without any free-form fields.
    #[must_use]
    pub fn to_json(self) -> Value {
        json!({
            "timestamp_unix_ms": timestamp_unix_ms(),
            "component": self.component.as_str(),
            "event": self.kind.as_str(),
            "activity": self.activity.as_str(),
            "failure_category": self.failure_category.map(FailureCategory::as_str),
            "operation_id": self.operation_id,
            "parent_operation_id": self.parent_operation_id,
            "target": self.target.map(DisplayControl::to_json),
            "duration_ms": self.duration_ms,
        })
    }
}

/// Session-scoped logger that can be cloned into worker threads.
///
/// The session is explicitly created by the caller. An error is remembered
/// for the rest of the session so an incomplete log cannot be mistaken for a
/// successful one after a later write succeeds.
#[derive(Clone)]
pub struct Session {
    state: Arc<SessionState>,
}

struct SessionState {
    file: EventFile,
    next_operation: AtomicU64,
    failed: AtomicBool,
    started: Instant,
    level: Level,
    displayed: Mutex<DisplayState>,
}

#[derive(Default)]
struct DisplayState {
    last: HashMap<DisplayControl, Option<String>>,
    seen: HashSet<DisplayControl>,
    in_frame: bool,
}

/// Creates sessions and records their classified events.
impl Session {
    fn from_file(file: File, component: Component, level: Level) -> io::Result<Self> {
        let session = Self {
            state: Arc::new(SessionState {
                file: EventFile::from_file(file),
                next_operation: AtomicU64::new(1),
                failed: AtomicBool::new(false),
                started: Instant::now(),
                level,
                displayed: Mutex::new(DisplayState::default()),
            }),
        };
        session.record(Event {
            component,
            kind: EventKind::Started,
            activity: Activity::Session,
            failure_category: None,
            operation_id: None,
            parent_operation_id: None,
            target: None,
            duration_ms: None,
        })?;
        session.write_record(&json!({
            "type": "coverage",
            "timestamp_unix_ms": timestamp_unix_ms(),
            "level": level.as_str(),
            "status": "incomplete",
            "reason": "instrumentation_in_progress",
        }))?;
        Ok(session)
    }

    /// Creates a uniquely named session file in a directory, creating the
    /// directory when necessary. Returns the session and its file path.
    ///
    /// # Errors
    ///
    /// Returns an error when directory creation or the initial record fails.
    pub fn create_in_directory(
        directory: &Path,
        component: Component,
    ) -> io::Result<(Self, PathBuf)> {
        Self::create_in_directory_with_level(directory, component, Level::Events)
    }

    /// Creates a uniquely named file with the selected diagnostic level.
    ///
    /// # Errors
    ///
    /// Returns an error when directory creation or the initial record fails.
    pub fn create_in_directory_with_level(
        directory: &Path,
        component: Component,
        level: Level,
    ) -> io::Result<(Self, PathBuf)> {
        let mut builder = DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(directory)?;
        #[cfg(unix)]
        let directory_handle = open_verified_log_directory(directory)?;
        #[cfg(not(unix))]
        verify_log_directory(directory)?;
        loop {
            let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let name = format!(
                "session-{stamp}-{}-{sequence}.sanikey-log.jsonl",
                std::process::id()
            );
            let path = directory.join(&name);
            let create_result = {
                #[cfg(unix)]
                {
                    create_session_file_at(&directory_handle, &name)
                }
                #[cfg(not(unix))]
                {
                    create_session_file(&path)
                }
            };
            match create_result {
                Ok(file) => {
                    return Self::from_file(file, component, level).map(|session| (session, path));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    /// Records a changed GUI text value at diagnostic level or higher.
    ///
    /// `None` denotes an explicitly identified secret and records only an
    /// omission marker. The control identifier must be a static application
    /// identifier, never a patient name or other user-supplied text.
    ///
    /// # Errors
    ///
    /// Returns an error on a poisoned lock, write or flush failure.
    pub fn record_displayed_text(
        &self,
        control: DisplayControl,
        text: Option<&str>,
    ) -> io::Result<()> {
        if self.state.level < Level::Diagnostic {
            return Ok(());
        }
        let mut displayed = self.state.displayed.lock().map_err(|_| {
            self.state.failed.store(true, Ordering::Relaxed);
            io::Error::other("display capture lock poisoned")
        })?;
        if displayed.in_frame {
            displayed.seen.insert(control);
        }
        if displayed
            .last
            .get(&control)
            .is_some_and(|last| last.as_deref() == text)
        {
            return Ok(());
        }
        let record = json!({"type": "displayed_text", "timestamp_unix_ms": timestamp_unix_ms(), "control": control.name, "index": control.index, "child_index": control.child_index, "leaf_index": control.leaf_index, "part": control.part.as_str(), "text": text, "secret_omitted": text.is_none()});
        self.write_record(&record)?;
        displayed.last.insert(control, text.map(str::to_owned));
        Ok(())
    }

    /// Records that native picker contents are unavailable to GUI capture.
    ///
    /// The record is omitted at Events level because it describes a diagnostic
    /// limitation rather than an operational event. The caller should invoke
    /// this once each time it opens the corresponding native dialog.
    ///
    /// # Errors
    ///
    /// Returns an error on a write or flush failure.
    pub fn record_native_dialog_omission(&self, dialog: NativeDialog) -> io::Result<()> {
        if self.state.level < Level::Diagnostic {
            return Ok(());
        }
        self.write_record(&json!({
            "type": "content_omitted",
            "timestamp_unix_ms": timestamp_unix_ms(),
            "source": "native_dialog",
            "dialog": dialog.as_str(),
            "reason": "external_component_unobservable",
        }))
    }

    /// Starts one GUI frame so later removals can be distinguished from unchanged text.
    ///
    /// # Errors
    ///
    /// Returns an error if a frame is already active or the capture lock is poisoned.
    pub fn begin_display_frame(&self) -> io::Result<()> {
        if self.state.level < Level::Diagnostic {
            return Ok(());
        }
        let mut display = self.state.displayed.lock().map_err(|_| {
            self.state.failed.store(true, Ordering::Relaxed);
            io::Error::other("display capture lock poisoned")
        })?;
        if display.in_frame {
            self.state.failed.store(true, Ordering::Relaxed);
            return Err(io::Error::other("display frame already active"));
        }
        display.seen.clear();
        display.in_frame = true;
        Ok(())
    }

    /// Ends one GUI frame and records controls no longer present.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid frame, a poisoned lock or a write failure.
    pub fn end_display_frame(&self) -> io::Result<()> {
        if self.state.level < Level::Diagnostic {
            return Ok(());
        }
        let mut display = self.state.displayed.lock().map_err(|_| {
            self.state.failed.store(true, Ordering::Relaxed);
            io::Error::other("display capture lock poisoned")
        })?;
        if !display.in_frame {
            self.state.failed.store(true, Ordering::Relaxed);
            return Err(io::Error::other("display frame not active"));
        }
        display.in_frame = false;
        let mut removed: Vec<_> = display
            .last
            .keys()
            .filter(|key| !display.seen.contains(key))
            .copied()
            .collect();
        removed.sort_by_key(|key| {
            (
                key.name,
                key.index,
                key.child_index,
                key.leaf_index,
                key.part,
            )
        });
        for control in removed {
            let record = json!({"type": "displayed_text_removed", "timestamp_unix_ms": timestamp_unix_ms(), "control": control.name, "index": control.index, "child_index": control.child_index, "leaf_index": control.leaf_index, "part": control.part.as_str()});
            self.write_record(&record)?;
            display.last.remove(&control);
        }
        Ok(())
    }

    /// Records an event and retains any failure for later inspection.
    ///
    /// # Errors
    ///
    /// Returns an error if the event cannot be written or flushed.
    fn record(&self, event: Event) -> io::Result<()> {
        let result = self.state.file.write_event(event);
        if result.is_err() {
            self.state.failed.store(true, Ordering::Relaxed);
        }
        result
    }

    fn write_record(&self, record: &Value) -> io::Result<()> {
        let result = self.state.file.write_json(record);
        if result.is_err() {
            self.state.failed.store(true, Ordering::Relaxed);
        }
        result
    }

    fn write_active_record(&self, record: &Value, transition: ActiveTransition) -> io::Result<()> {
        let result = self.state.file.write_json_inner(record, transition);
        if result.is_err() {
            self.state.failed.store(true, Ordering::Relaxed);
        }
        result
    }

    /// Indicates a write failure or an operation or call without an outcome.
    #[must_use]
    pub fn has_failed(&self) -> bool {
        self.state.failed.load(Ordering::Relaxed)
    }

    /// Marks an application shutdown that still has active worker threads.
    ///
    /// # Errors
    ///
    /// Returns an error if the marker cannot be written.
    pub fn shutdown_pending(&self, component: Component) -> io::Result<()> {
        self.record(Event {
            component,
            kind: EventKind::ShutdownPending,
            activity: Activity::Session,
            failure_category: None,
            operation_id: None,
            parent_operation_id: None,
            target: None,
            duration_ms: Some(
                u64::try_from(self.state.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            ),
        })
    }

    /// Records the session end after its operations have finished.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal event cannot be written.
    pub fn finish(&self, component: Component) -> io::Result<()> {
        let duration_ms =
            u64::try_from(self.state.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let event = Event {
            component,
            kind: EventKind::Finished,
            activity: Activity::Session,
            failure_category: None,
            operation_id: None,
            parent_operation_id: None,
            target: None,
            duration_ms: Some(duration_ms),
        };
        let result = self.state.file.finish_event(event);
        if result.is_err() {
            self.state.failed.store(true, Ordering::Relaxed);
        }
        result
    }

    /// Begins a timed operation with a session-local identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if its start event cannot be written.
    pub fn start_operation(
        &self,
        component: Component,
        activity: Activity,
    ) -> io::Result<Operation> {
        self.start_operation_with_target(component, activity, None)
    }

    /// Begins an action on a statically identified GUI control.
    ///
    /// # Errors
    ///
    /// Returns an error if its start record cannot be written.
    pub fn start_control_action(
        &self,
        activity: Activity,
        target: DisplayControl,
    ) -> io::Result<Operation> {
        self.start_operation_with_target(Component::Desktop, activity, Some(target))
    }

    fn start_operation_with_target(
        &self,
        component: Component,
        activity: Activity,
        target: Option<DisplayControl>,
    ) -> io::Result<Operation> {
        let id = self.state.next_operation.fetch_add(1, Ordering::Relaxed);
        let parent_operation_id = context::current_operation_id(self);
        let event = Event {
            component,
            kind: EventKind::Started,
            activity,
            failure_category: None,
            operation_id: Some(id),
            parent_operation_id,
            target,
            duration_ms: None,
        };
        self.write_active_record(&event.to_json(), ActiveTransition::Start(id))?;
        Ok(Operation {
            session: self.clone(),
            component,
            activity,
            id,
            parent_operation_id,
            target,
            started: Instant::now(),
            finished: false,
        })
    }
}

#[cfg(unix)]
fn open_verified_log_directory(directory: &Path) -> io::Result<std::os::fd::OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let handle = if let Some(name) = directory.file_name() {
        let parent_path = directory
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = open(
            parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        openat(&parent, name, flags, Mode::empty())?
    } else {
        open(directory, flags, Mode::empty())?
    };
    let metadata = fstat(&handle)?;
    if !FileType::from_raw_mode(metadata.st_mode).is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la directory dei log deve essere una directory reale",
        ));
    }
    if metadata.st_mode & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "la directory dei log è scrivibile da altri utenti o gruppi",
        ));
    }
    Ok(handle)
}

#[cfg(unix)]
fn create_session_file_at(directory: &std::os::fd::OwnedFd, name: &str) -> io::Result<File> {
    let file = openat(
        directory,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?;
    Ok(File::from(file))
}

#[cfg(not(unix))]
fn verify_log_directory(directory: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !metadata.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la directory dei log deve essere una directory reale",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_session_file(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    options.open(path)
}

/// Timed operation tied to one logging session.
pub struct Operation {
    session: Session,
    component: Component,
    activity: Activity,
    id: u64,
    parent_operation_id: Option<u64>,
    target: Option<DisplayControl>,
    started: Instant,
    finished: bool,
}

/// Terminal outcome of a timed operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Completed successfully.
    Succeeded,
    /// Failed.
    Failed,
    /// Failed with a known non-sensitive category.
    FailedWith(FailureCategory),
    /// Cancelled.
    Cancelled,
}

impl Operation {
    /// Records a change in the coarse operation phase.
    ///
    /// # Errors
    ///
    /// Returns an error if the phase record cannot be written.
    pub fn phase(&self, phase: ProgressPhase) -> io::Result<()> {
        self.session.write_record(&json!({
            "type": "operation_phase",
            "timestamp_unix_ms": timestamp_unix_ms(),
            "component": self.component.as_str(),
            "activity": self.activity.as_str(),
            "operation_id": self.id,
            "parent_operation_id": self.parent_operation_id,
            "target": self.target.map(DisplayControl::to_json),
            "phase": phase.as_str(),
        }))
    }

    /// Records numeric progress without accepting private free-form text.
    ///
    /// # Errors
    ///
    /// Returns an error if the progress record cannot be written.
    pub fn progress(
        &self,
        phase: ProgressPhase,
        completed: u64,
        total: Option<u64>,
    ) -> io::Result<()> {
        self.session.write_record(&json!({
            "type": "operation_progress",
            "timestamp_unix_ms": timestamp_unix_ms(),
            "component": self.component.as_str(),
            "activity": self.activity.as_str(),
            "operation_id": self.id,
            "parent_operation_id": self.parent_operation_id,
            "target": self.target.map(DisplayControl::to_json),
            "phase": phase.as_str(),
            "completed": completed,
            "total": total,
        }))
    }

    /// Records the terminal event and elapsed duration.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal event cannot be written.
    pub fn finish(mut self, outcome: Outcome) -> io::Result<()> {
        let duration_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (kind, failure_category) = match outcome {
            Outcome::Succeeded => (EventKind::Succeeded, None),
            Outcome::Failed => (EventKind::Failed, Some(FailureCategory::Other)),
            Outcome::FailedWith(category) => (EventKind::Failed, Some(category)),
            Outcome::Cancelled => (EventKind::Cancelled, None),
        };
        self.finished = true;
        self.write_terminal(kind, failure_category, duration_ms)
    }

    fn write_terminal(
        &self,
        kind: EventKind,
        failure_category: Option<FailureCategory>,
        duration_ms: u64,
    ) -> io::Result<()> {
        let event = Event {
            component: self.component,
            kind,
            activity: self.activity,
            failure_category,
            operation_id: Some(self.id),
            parent_operation_id: self.parent_operation_id,
            target: self.target,
            duration_ms: Some(duration_ms),
        };
        self.session
            .write_active_record(&event.to_json(), ActiveTransition::End(self.id))
    }
}

/// Marks an operation incomplete when it is dropped without finishing.
impl Drop for Operation {
    fn drop(&mut self) {
        if !self.finished {
            let duration = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let _ = self.write_terminal(
                EventKind::Unfinished,
                Some(FailureCategory::Other),
                duration,
            );
            self.session.state.failed.store(true, Ordering::Relaxed);
        }
    }
}

/// A single session's event file, shared safely between worker threads.
///
/// This sink only accepts [`Event`] values; richer records require a separate
/// reviewed interface. A caller must surface creation and write failures.
struct EventFile {
    state: Mutex<FileState>,
}

struct FileState {
    file: File,
    failed: bool,
    closed: bool,
    next_sequence: u64,
    active: HashSet<u64>,
}

#[derive(Clone, Copy)]
enum ActiveTransition {
    None,
    Start(u64),
    End(u64),
    Close,
}

/// Serializes session records and remembers write failures.
impl EventFile {
    fn from_file(file: File) -> Self {
        Self {
            state: Mutex::new(FileState {
                file,
                failed: false,
                closed: false,
                next_sequence: 0,
                active: HashSet::new(),
            }),
        }
    }

    /// Writes and flushes one complete JSON line.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned or writing or flushing fails.
    pub fn write_event(&self, event: Event) -> io::Result<()> {
        self.write_json(&event.to_json())
    }

    fn write_json(&self, value: &Value) -> io::Result<()> {
        self.write_json_inner(value, ActiveTransition::None)
    }

    fn finish_event(&self, event: Event) -> io::Result<()> {
        self.write_json_inner(&event.to_json(), ActiveTransition::Close)
    }

    fn write_json_inner(&self, value: &Value, transition: ActiveTransition) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("log lock poisoned"))?;
        if state.failed {
            return Err(io::Error::other(
                "log incomplete after previous write failure",
            ));
        }
        if state.closed {
            return Err(io::Error::other("log session already closed"));
        }
        match transition {
            ActiveTransition::Close if !state.active.is_empty() => {
                return Err(io::Error::other("log has active operations or calls"));
            }
            ActiveTransition::Start(id) if state.active.contains(&id) => {
                return Err(io::Error::other("duplicate log activity"));
            }
            ActiveTransition::End(id) if !state.active.contains(&id) => {
                return Err(io::Error::other("unknown log activity"));
            }
            _ => {}
        }
        let Some(object) = value.as_object() else {
            return Err(io::Error::other("log record must be an object"));
        };
        if object.contains_key("sequence") {
            return Err(io::Error::other("reserved log sequence field"));
        }
        let sequence = state.next_sequence;
        let next = sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("log sequence exhausted"))?;
        let result = (|| {
            write!(state.file, "{{\"sequence\":{sequence}")?;
            for (key, value) in object {
                write!(state.file, ",")?;
                serde_json::to_writer(&mut state.file, key).map_err(io::Error::other)?;
                write!(state.file, ":")?;
                serde_json::to_writer(&mut state.file, value).map_err(io::Error::other)?;
            }
            writeln!(state.file, "}}")?;
            state.file.flush()
        })();
        if result.is_err() {
            state.failed = true;
        } else {
            state.next_sequence = next;
        }
        if result.is_ok() {
            match transition {
                ActiveTransition::Start(id) => {
                    state.active.insert(id);
                }
                ActiveTransition::End(id) => {
                    state.active.remove(&id);
                }
                ActiveTransition::Close => state.closed = true,
                ActiveTransition::None => {}
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Activity, Component, DisplayControl, Event, EventFile, EventKind, Level, NativeDialog,
        Outcome, Session,
    };
    #[cfg(unix)]
    use super::{create_session_file_at, open_verified_log_directory};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn xdg_state_paths_ignore_relative_values_and_use_home_fallback() {
        use super::xdg_state_directory;
        assert_eq!(
            xdg_state_directory(
                Some(PathBuf::from("relative")),
                Some(PathBuf::from("/synthetic/user"))
            ),
            Some(PathBuf::from("/synthetic/user/.local/state"))
        );
        assert_eq!(
            xdg_state_directory(
                Some(PathBuf::from("/state")),
                Some(PathBuf::from("/synthetic/user"))
            ),
            Some(PathBuf::from("/state"))
        );
        assert_eq!(
            xdg_state_directory(None, Some(PathBuf::from("relative"))),
            None
        );
    }

    #[test]
    fn event_format_has_only_fixed_identifiers_and_numeric_context() {
        let event = Event {
            component: Component::Desktop,
            kind: EventKind::Failed,
            activity: Activity::Build,
            failure_category: Some(super::FailureCategory::Other),
            operation_id: Some(7),
            parent_operation_id: None,
            target: None,
            duration_ms: Some(42),
        };
        let mut encoded = event.to_json();
        assert!(encoded["timestamp_unix_ms"].is_u64());
        encoded.as_object_mut().unwrap().remove("timestamp_unix_ms");
        assert_eq!(
            encoded,
            json!({"component": "desktop", "event": "failed", "activity": "build", "failure_category": "other", "operation_id": 7, "parent_operation_id": null, "target": null, "duration_ms": 42})
        );
    }

    #[test]
    fn session_file_is_exclusive_and_preserves_concurrent_records() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-log-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) = Session::create_in_directory(&directory, Component::Core)
            .expect("create synthetic session");
        let session = Arc::new(session);
        let (second, second_path) =
            Session::create_in_directory(&directory, Component::Core).expect("second session");
        assert_ne!(path, second_path);
        drop(second);
        fs::remove_file(second_path).expect("remove second session file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
        }
        let threads: Vec<_> = (0..4)
            .map(|id| {
                let session = Arc::clone(&session);
                std::thread::spawn(move || {
                    session
                        .record(Event {
                            component: Component::Core,
                            kind: EventKind::Started,
                            activity: Activity::Build,
                            failure_category: None,
                            operation_id: Some(id),
                            parent_operation_id: None,
                            target: None,
                            duration_ms: None,
                        })
                        .expect("write synthetic event");
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("worker");
        }
        drop(session);
        let contents = fs::read_to_string(&path).expect("read synthetic log");
        let records: Vec<serde_json::Value> = contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("complete JSON record"))
            .collect();
        assert_eq!(records.len(), 6);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record["sequence"], index as u64);
        }
        for id in 0..4 {
            assert!(records.iter().any(|record| record["operation_id"] == id));
        }
        fs::remove_file(path).expect("remove synthetic log");
        fs::remove_dir(directory).expect("remove synthetic directory");
    }

    #[test]
    fn operations_from_worker_threads_have_correlated_start_and_finish() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-session-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory(&directory, Component::Desktop).expect("session");
        let worker_session = session.clone();
        std::thread::spawn(move || {
            worker_session
                .start_operation(Component::Core, Activity::Build)
                .expect("start")
                .finish(Outcome::Succeeded)
                .expect("finish");
        })
        .join()
        .expect("worker");
        assert!(!session.has_failed());
        session.finish(Component::Desktop).expect("finish session");
        drop(session);
        let contents = fs::read_to_string(&path).expect("read session");
        let records: Vec<serde_json::Value> = contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON"))
            .collect();
        assert_eq!(records.len(), 5);
        assert_eq!(records[1]["status"], "incomplete");
        assert_eq!(records[2]["operation_id"], records[3]["operation_id"]);
        assert_eq!(records[3]["event"], "succeeded");
        assert!(records[3]["duration_ms"].is_u64());
        assert_eq!(records[4]["event"], "finished");
        fs::remove_file(path).expect("remove synthetic log");
        fs::remove_dir(directory).expect("remove session directory");
    }

    #[test]
    fn separate_sessions_create_distinct_files() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-session-directory-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (first, first_path) = Session::create_in_directory(&directory, Component::Cli).unwrap();
        let (second, second_path) =
            Session::create_in_directory(&directory, Component::Cli).unwrap();
        assert_ne!(first_path, second_path);
        drop((first, second));
        fs::remove_file(first_path).unwrap();
        fs::remove_file(second_path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_existing_directory_writable_by_others_without_changing_it() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-insecure-dir-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o777)).unwrap();
        let error = Session::create_in_directory(&directory, Component::Core)
            .err()
            .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o777
        );
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_as_log_directory() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "sanikey-log-dir-link-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&root).unwrap();
        let target = root.join("target");
        let link = root.join("link");
        fs::create_dir(&target).unwrap();
        symlink(&target, &link).unwrap();
        assert!(Session::create_in_directory(&link, Component::Core).is_err());
        let link_with_separator = PathBuf::from(format!("{}/", link.display()));
        assert!(open_verified_log_directory(&link_with_separator).is_err());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        fs::remove_file(link).unwrap();
        fs::remove_dir(target).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn closed_session_rejects_late_events_and_second_finish() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-close-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) = Session::create_in_directory(&directory, Component::Core).unwrap();
        session.finish(Component::Core).unwrap();
        assert!(session.finish(Component::Core).is_err());
        assert!(
            session
                .start_operation(Component::Core, Activity::Build)
                .is_err()
        );
        assert!(session.has_failed());
        drop(session);
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(
            content
                .lines()
                .filter(|line| line.contains("\"event\":\"finished\""))
                .count(),
            1
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn active_operation_prevents_close_and_drop_records_unfinished() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-unfinished-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) = Session::create_in_directory(&directory, Component::Core).unwrap();
        let operation = session
            .start_operation(Component::Core, Activity::Build)
            .unwrap();
        session.shutdown_pending(Component::Core).unwrap();
        assert!(session.finish(Component::Core).is_err());
        drop(operation);
        assert!(session.has_failed());
        session.finish(Component::Core).unwrap();
        drop(session);
        let log = fs::read_to_string(&path).unwrap();
        assert_eq!(log.matches("\"event\":\"unfinished\"").count(), 1);
        assert_eq!(log.matches("\"event\":\"shutdown_pending\"").count(), 1);
        assert_eq!(log.matches("\"event\":\"finished\"").count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn control_actions_include_only_static_target_and_numeric_indices() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-action-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) = Session::create_in_directory(&directory, Component::Desktop).unwrap();
        let target = DisplayControl::indexed("person_heading", 3, None);
        session
            .start_control_action(Activity::GuiPanelToggled, target)
            .unwrap()
            .finish(Outcome::Succeeded)
            .unwrap();
        session.finish(Component::Desktop).unwrap();
        drop(session);
        let records: Vec<serde_json::Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let action = records
            .iter()
            .find(|item| item["activity"] == "gui_panel_toggled")
            .unwrap();
        assert_eq!(action["target"]["name"], "person_heading");
        assert_eq!(action["target"]["index"], 3);
        assert_eq!(action["target"].as_object().unwrap().len(), 5);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn diagnostic_capture_records_changes_but_not_identified_secrets() {
        for level in [Level::Events, Level::Diagnostic, Level::Trace] {
            let directory = std::env::temp_dir().join(format!(
                "sanikey-presentation-{}-{:?}-{level:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let (session, path) =
                Session::create_in_directory_with_level(&directory, Component::Desktop, level)
                    .unwrap();
            session
                .record_displayed_text(
                    DisplayControl::new("status"),
                    Some("primo valore sintetico"),
                )
                .unwrap();
            session
                .record_displayed_text(
                    DisplayControl::new("status"),
                    Some("primo valore sintetico"),
                )
                .unwrap();
            session
                .record_displayed_text(
                    DisplayControl::new("status"),
                    Some("secondo valore sintetico"),
                )
                .unwrap();
            session
                .record_displayed_text(DisplayControl::new("password"), None)
                .unwrap();
            session
                .record_displayed_text(
                    DisplayControl::indexed("person", 0, None),
                    Some("persona sintetica"),
                )
                .unwrap();
            session
                .record_displayed_text(
                    DisplayControl::indexed("person", 1, None),
                    Some("persona sintetica"),
                )
                .unwrap();
            drop(session);
            let content = fs::read_to_string(&path).unwrap();
            if level == Level::Events {
                assert!(!content.contains("valore sintetico"));
                assert!(!content.contains("password"));
            } else {
                assert_eq!(content.matches("primo valore sintetico").count(), 1);
                assert_eq!(content.matches("secondo valore sintetico").count(), 1);
                assert!(content.contains("\"secret_omitted\":true"));
                assert_eq!(content.matches("persona sintetica").count(), 2);
            }
            fs::remove_file(path).unwrap();
            fs::remove_dir(directory).unwrap();
        }
    }

    #[test]
    fn native_dialog_omissions_are_typed_diagnostic_records_without_private_data() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-native-dialog-omission-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (events_session, events_path) =
            Session::create_in_directory_with_level(&directory, Component::Desktop, Level::Events)
                .expect("sessione eventi sintetica");
        events_session
            .record_native_dialog_omission(NativeDialog::ProfileFile)
            .expect("omissione ignorata a livello eventi");
        events_session
            .finish(Component::Desktop)
            .expect("fine sessione eventi");
        let events = fs::read_to_string(&events_path).expect("log eventi");
        assert!(!events.contains("content_omitted"));

        let (diagnostic_session, diagnostic_path) = Session::create_in_directory_with_level(
            &directory,
            Component::Desktop,
            Level::Diagnostic,
        )
        .expect("sessione diagnostica sintetica");
        diagnostic_session
            .record_native_dialog_omission(NativeDialog::ProfileFile)
            .expect("omissione picker profilo");
        diagnostic_session
            .record_native_dialog_omission(NativeDialog::PathFolder)
            .expect("omissione picker cartella");
        diagnostic_session
            .record_native_dialog_omission(NativeDialog::UsbTargetFolder)
            .expect("omissione picker destinazione USB");
        diagnostic_session
            .finish(Component::Desktop)
            .expect("fine sessione diagnostica");
        let diagnostic = fs::read_to_string(&diagnostic_path).expect("log diagnostico");
        let omissions: Vec<Value> = diagnostic
            .lines()
            .filter_map(|line| {
                let record: Value = serde_json::from_str(line).expect("record JSON");
                (record["type"] == "content_omitted").then_some(record)
            })
            .collect();
        assert_eq!(omissions.len(), 3);
        assert_eq!(omissions[0]["source"], "native_dialog");
        assert_eq!(omissions[0]["dialog"], "profile_file");
        assert_eq!(omissions[0]["reason"], "external_component_unobservable");
        assert_eq!(omissions[1]["dialog"], "path_folder");
        assert_eq!(omissions[2]["dialog"], "usb_target_folder");
        assert!(!diagnostic.contains("private"));

        let (trace_session, trace_path) =
            Session::create_in_directory_with_level(&directory, Component::Desktop, Level::Trace)
                .expect("sessione trace sintetica");
        trace_session
            .record_native_dialog_omission(NativeDialog::ProfileFile)
            .expect("omissione picker al livello trace");
        trace_session
            .finish(Component::Desktop)
            .expect("fine sessione trace");
        let trace = fs::read_to_string(&trace_path).expect("log trace");
        assert!(trace.contains("external_component_unobservable"));

        fs::remove_file(events_path).expect("rimozione log eventi");
        fs::remove_file(diagnostic_path).expect("rimozione log diagnostico");
        fs::remove_file(trace_path).expect("rimozione log trace");
        fs::remove_dir(directory).expect("rimozione directory sintetica");
    }

    #[test]
    fn display_frame_records_removed_and_reappearing_controls() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-frame-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) = Session::create_in_directory_with_level(
            &directory,
            Component::Desktop,
            Level::Diagnostic,
        )
        .unwrap();
        let control = DisplayControl::new("synthetic");
        session.begin_display_frame().unwrap();
        session
            .record_displayed_text(control, Some("valore sintetico"))
            .unwrap();
        session.end_display_frame().unwrap();
        session.begin_display_frame().unwrap();
        session.end_display_frame().unwrap();
        session.begin_display_frame().unwrap();
        session
            .record_displayed_text(control, Some("valore sintetico"))
            .unwrap();
        session.end_display_frame().unwrap();
        drop(session);
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("valore sintetico").count(), 2);
        assert_eq!(content.matches("displayed_text_removed").count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn write_failure_is_remembered_for_the_session() {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicBool, AtomicU64};
        use std::time::Instant;

        let session = Session {
            state: Arc::new(super::SessionState {
                file: EventFile::from_file(
                    fs::OpenOptions::new()
                        .write(true)
                        .open("/dev/full")
                        .unwrap(),
                ),
                next_operation: AtomicU64::new(1),
                failed: AtomicBool::new(false),
                started: Instant::now(),
                level: super::Level::Events,
                displayed: Mutex::new(super::DisplayState::default()),
            }),
        };
        assert!(
            session
                .start_operation(Component::Cli, Activity::Command)
                .is_err()
        );
        assert!(session.has_failed());
        assert!(session.finish(Component::Cli).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn relative_session_file_creation_refuses_a_symbolic_link() {
        use std::os::unix::fs::symlink;
        let directory = std::env::temp_dir().join(format!(
            "sanikey-log-link-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&directory).expect("create synthetic directory");
        let target = directory.join("missing");
        let link = directory.join("session-link");
        symlink(&target, &link).expect("create synthetic link");
        let directory_handle = open_verified_log_directory(&directory).expect("open directory");
        assert!(create_session_file_at(&directory_handle, "session-link").is_err());
        assert!(!target.exists());
        drop(directory_handle);
        fs::remove_file(link).expect("remove synthetic link");
        fs::remove_dir(directory).expect("remove synthetic directory");
    }

    #[cfg(unix)]
    #[test]
    fn relative_session_file_creation_is_exclusive() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-log-excl-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&directory).expect("create synthetic directory");
        let directory_handle = open_verified_log_directory(&directory).expect("open directory");
        let file =
            create_session_file_at(&directory_handle, "session.jsonl").expect("create first file");
        let error =
            create_session_file_at(&directory_handle, "session.jsonl").expect_err("existing file");
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        drop((file, directory_handle));
        fs::remove_file(directory.join("session.jsonl")).expect("remove session file");
        fs::remove_dir(directory).expect("remove synthetic directory");
    }

    #[cfg(unix)]
    #[test]
    fn open_directory_handle_pins_session_file_creation_after_path_replacement() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "sanikey-log-dir-replaced-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&root).expect("create synthetic root");
        let selected = root.join("selected");
        let moved = root.join("selected-moved");
        let attacker_target = root.join("attacker-target");
        fs::create_dir(&selected).expect("create selected directory");
        fs::create_dir(&attacker_target).expect("create attacker target");
        let directory_handle = open_verified_log_directory(&selected).expect("open selected dir");

        fs::rename(&selected, &moved).expect("move selected directory");
        symlink(&attacker_target, &selected).expect("replace path with symlink");
        let file = create_session_file_at(&directory_handle, "session.jsonl")
            .expect("create through pinned directory handle");
        drop(file);

        assert!(moved.join("session.jsonl").is_file());
        assert_eq!(fs::read_dir(&attacker_target).unwrap().count(), 0);
        drop(directory_handle);
        fs::remove_file(selected).expect("remove replacement symlink");
        fs::remove_file(moved.join("session.jsonl")).expect("remove created file");
        fs::remove_dir(moved).expect("remove moved directory");
        fs::remove_dir(attacker_target).expect("remove attacker target");
        fs::remove_dir(root).expect("remove synthetic root");
    }
}
