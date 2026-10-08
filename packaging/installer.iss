; Windows upgrade installer for existing 1.3.x AppId installations.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\windows-installer-stage"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif
#define AppName "S.T.A.L.K.E.R. Save Editor"
#define AppExeName "sse-shell.exe"

[Setup]
AppId={{5E973B9B-8344-4821-B86B-25B3E75A384F}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Dmitriy-DE
AppPublisherURL=https://github.com/Dmitriy-DE/S.T.A.L.K.E.R.-Save-Editor
AppSupportURL=https://github.com/Dmitriy-DE/S.T.A.L.K.E.R.-Save-Editor/issues
DefaultDirName={autopf}\StalkerSaveEditor
DefaultGroupName={#AppName}
AllowNoIcons=yes
OutputDir={#OutputDir}
OutputBaseFilename=SaveEditor-windows-x86_64-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\{#AppExeName}
CloseApplications=yes

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#SourceDir}\sse-shell.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\stalker-save.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\BUILD_MANIFEST.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\INSTALLER_MARKER"; DestDir: "{app}"; Flags: ignoreversion

; Remove executable names from the previous C# install while preserving other user files.
[InstallDelete]
Type: files; Name: "{app}\StalkerSaveEditor.exe"
Type: files; Name: "{app}\stalker-save-editor-cli.exe"
Type: files; Name: "{app}\stalker_ooz.dll"

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\INSTALLER_MARKER"
