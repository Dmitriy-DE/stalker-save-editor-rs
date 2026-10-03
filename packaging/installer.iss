; S.T.A.L.K.E.R. Save Editor Inno Setup Script
[Setup]
AppName=S.T.A.L.K.E.R. Save Editor
AppVersion=2.0.0-dev
DefaultDirName={autopf}\S.T.A.L.K.E.R. Save Editor
DefaultGroupName=S.T.A.L.K.E.R. Save Editor
OutputDir=/home/dmytro/Projects/save-editor-rs/dist
OutputBaseFilename=stalker-save-editor-windows-installer
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible

[Files]
Source: "/tmp/sse-win-build-1LEHVw\package\stalker-save.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "/tmp/sse-win-build-1LEHVw\package\BUILD_MANIFEST.json"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\S.T.A.L.K.E.R. Save Editor"; Filename: "{app}\stalker-save.exe"
Name: "{commondesktop}\S.T.A.L.K.E.R. Save Editor"; Filename: "{app}\stalker-save.exe"

[Run]
Filename: "{app}\stalker-save.exe"; Description: "{cm:LaunchProgram,S.T.A.L.K.E.R. Save Editor}"; Flags: nowait postinstall skipifsilent
