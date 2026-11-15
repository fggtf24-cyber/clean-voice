; Inno Setup script for Clean Voice. Built by release.ps1:
;   ISCC.exe /DAppVersion=0.1.0 installer\clean_voice.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{EA2F4AC9-97F1-456C-868E-F6F5FFE331DF}
AppName=Clean Voice
AppVersion={#AppVersion}
AppVerName=Clean Voice {#AppVersion}
AppPublisher=Clean Voice
AppPublisherURL=https://github.com/fggtf24-cyber/clean-voice
DefaultDirName={commonpf64}\Clean Voice
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=CleanVoice-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
; Offers to close Premiere/Resolve/Ableton/OBS if they hold the old plugin file.
CloseApplications=yes
RestartApplications=no
LicenseFile=..\LICENSE
UninstallDisplayName=Clean Voice {#AppVersion}
WizardStyle=modern

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"

[Files]
Source: "..\target\bundled\Clean Voice.vst3\*"; DestDir: "{commoncf64}\VST3\Clean Voice.vst3"; Flags: recursesubdirs ignoreversion
Source: "..\target\bundled\Clean Voice.clap"; DestDir: "{commoncf64}\CLAP"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[UninstallDelete]
Type: filesandordirs; Name: "{commoncf64}\VST3\Clean Voice.vst3"
