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
$rgb = { param($r, $g, $b) [Drawing.Color]::FromArgb($r, $g, $b) }
$ink = & $rgb 44 44 42; $face = & $rgb 233 226 207; $btn = & $rgb 221 213 189; $edge = & $rgb 95 94 90
$f = New-Object Windows.Forms.Form
$f.FormBorderStyle = 'None'; $f.TopMost = $true; $f.StartPosition = 'CenterScreen'; $f.ShowInTaskbar = $false
$f.ClientSize = New-Object Drawing.Size(340, 128); $f.BackColor = $ink; $f.Padding = New-Object Windows.Forms.Padding(3)
$f.Text = 'Clean Voice'
$pn = New-Object Windows.Forms.Panel; $pn.Dock = 'Fill'; $pn.BackColor = $face; $f.Controls.Add($pn)
$h = New-Object Windows.Forms.Label; $h.Text = 'Clean Voice'; $h.Font = New-Object Drawing.Font('Segoe UI', 12)
$h.ForeColor = $ink; $h.SetBounds(12, 8, 300, 24)
$l = New-Object Windows.Forms.Label; $l.Text = $env:CV_PROMPT; $l.Font = New-Object Drawing.Font('Segoe UI', 9)
$l.ForeColor = (& $rgb 153 60 29); $l.SetBounds(13, 34, 300, 18)
$t = New-Object Windows.Forms.TextBox; $t.SetBounds(12, 56, 310, 26); $t.MaxLength = 60
$t.BackColor = $ink; $t.ForeColor = (& $rgb 239 159 39); $t.BorderStyle = 'FixedSingle'; $t.Font = New-Object Drawing.Font('Segoe UI', 11)
$mk = { param($text, $x, $res) $b = New-Object Windows.Forms.Button; $b.Text = $text; $b.SetBounds($x, 90, 90, 26)
  $b.FlatStyle = 'Flat'; $b.BackColor = $btn; $b.ForeColor = $ink; $b.FlatAppearance.BorderColor = $edge
  $b.Font = New-Object Drawing.Font('Segoe UI', 9); $b.DialogResult = $res; $b }
$o = & $mk $env:CV_OK 136 'OK'; $c = & $mk $env:CV_CANCEL 232 'Cancel'
$pn.Controls.AddRange(@($h, $l, $t, $o, $c)); $f.AcceptButton = $o; $f.CancelButton = $c
$drag = { if ($_.Button -eq 'Left') { [void][Win]::ReleaseCapture(); [void][Win]::SendMessage($f.Handle, 0xA1, 2, 0) } }
Add-Type 'using System;using System.Runtime.InteropServices;public class Win{[DllImport("user32.dll")]public static extern bool ReleaseCapture();[DllImport("user32.dll")]public static extern IntPtr SendMessage(IntPtr h,int m,int w,int l);}'
$pn.Add_MouseDown($drag); $h.Add_MouseDown($drag)
$f.Add_Shown({ $f.Activate(); $t.Focus() })
if ($f.ShowDialog() -eq 'OK') { [Console]::Out.Write($t.Text) }
"#;

/// Opens a modal text prompt in its own window, styled like the plugin (non-blocking for the
/// caller). `ok`/`cancel` are the localized button captions.
pub fn ask_text(prompt: &str, ok: &str, cancel: &str) -> Pending {
    let out: Pending = Arc::new(Mutex::new(None));
    let res = out.clone();
    let (prompt, ok, cancel) = (prompt.to_string(), ok.to_string(), cancel.to_string());
    std::thread::spawn(move || {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", DIALOG_PS])
            .env("CV_PROMPT", prompt)
            .env("CV_OK", ok)
            .env("CV_CANCEL", cancel)
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
