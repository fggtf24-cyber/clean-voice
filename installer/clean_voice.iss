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
; {app} = the VST3 folder the user picks (standard one by default, "Browse..." to change).
; On updates the previously chosen folder is reused and the page is skipped.
DefaultDirName={commoncf64}\VST3
DisableDirPage=auto
AppendDefaultDirName=no
DirExistsWarning=no
UsePreviousAppDir=yes
; Keep the uninstaller + license out of the VST3 folder.
UninstallFilesDir={commonpf64}\Clean Voice
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

[Messages]
en.SelectDirLabel3=Choose the folder for the Clean Voice VST3 plugin. The standard VST3 folder is recommended: Premiere, Resolve and Ableton find the plugin there automatically.
ru.SelectDirLabel3=Выберите папку для VST3-плагина Clean Voice. Рекомендуется стандартная папка VST3: Premiere, Resolve и Ableton находят плагин там автоматически.
en.SelectDirBrowseLabel=Click Next to continue, or Browse to pick another folder.
ru.SelectDirBrowseLabel=Нажмите «Далее», чтобы продолжить, или «Обзор», чтобы выбрать другую папку.

[Files]
Source: "..\target\bundled\Clean Voice.vst3\*"; DestDir: "{app}\Clean Voice.vst3"; Flags: recursesubdirs ignoreversion
Source: "..\target\bundled\Clean Voice.clap"; DestDir: "{commoncf64}\CLAP"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{commonpf64}\Clean Voice"; Flags: ignoreversion

[UninstallDelete]
Type: filesandordirs; Name: "{app}\Clean Voice.vst3"
