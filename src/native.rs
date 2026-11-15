//! Small OS helpers that must work regardless of the host's keyboard handling.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// No console window for helper processes.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Pending text input: `None` while the dialog is open, then `Some(None)` = cancelled,
/// `Some(Some(text))` = entered.
pub type Pending = Arc<Mutex<Option<Option<String>>>>;

// Many hosts (Premiere, Resolve...) swallow key presses for their own shortcuts, so text typed
// into the plugin window never arrives. A separate top-level WinForms dialog always gets the
// keyboard.
// ponytail: spawns PowerShell (always present on Win10/11); upgrade = native Win32 dialog.
const DIALOG_PS: &str = r#"
[Console]::OutputEncoding = [Text.Encoding]::UTF8
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object Windows.Forms.Form
$f.Text = 'Clean Voice'; $f.TopMost = $true; $f.StartPosition = 'CenterScreen'
$f.FormBorderStyle = 'FixedDialog'; $f.MinimizeBox = $false; $f.MaximizeBox = $false
$f.ClientSize = New-Object Drawing.Size(320, 96)
$l = New-Object Windows.Forms.Label; $l.Text = $env:CV_PROMPT; $l.SetBounds(12, 10, 296, 18)
$t = New-Object Windows.Forms.TextBox; $t.SetBounds(12, 32, 296, 22); $t.MaxLength = 60
$o = New-Object Windows.Forms.Button; $o.Text = 'OK'; $o.SetBounds(148, 62, 75, 24); $o.DialogResult = 'OK'
$c = New-Object Windows.Forms.Button; $c.Text = 'Cancel'; $c.SetBounds(233, 62, 75, 24); $c.DialogResult = 'Cancel'
$f.Controls.AddRange(@($l, $t, $o, $c)); $f.AcceptButton = $o; $f.CancelButton = $c
$f.Add_Shown({ $f.Activate(); $t.Focus() })
if ($f.ShowDialog() -eq 'OK') { [Console]::Out.Write($t.Text) }
"#;

/// Opens a modal text prompt in its own window (non-blocking for the caller).
pub fn ask_text(prompt: &str) -> Pending {
    let out: Pending = Arc::new(Mutex::new(None));
    let res = out.clone();
    let prompt = prompt.to_string();
    std::thread::spawn(move || {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", DIALOG_PS])
            .env("CV_PROMPT", prompt)
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        let text = cmd
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        *res.lock().unwrap() = Some(text);
    });
    out
}

/// Opens a file/URL with the shell (UAC elevation prompts work, unlike a plain spawn).
pub fn shell_open(target: &str) {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "start", "", target]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let _ = cmd.spawn();
}
