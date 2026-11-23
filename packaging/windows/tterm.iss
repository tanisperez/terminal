; Inno Setup 6 script for Tanis Terminal. Compile with ISCC, defines on the command line:
;   ISCC.exe /DAppVersion=0.1.0-rc.1 /DSourceExe=C:\path\tterm.exe /DOutputDir=C:\out tterm.iss
;   AppVersion  full version, may carry a pre-release suffix (X.Y.Z or X.Y.Z-rc.1)
;   SourceExe   path to the release tterm.exe
;   OutputDir   folder for tterm-<version>-windows-x86_64-setup.exe (default: .\Output)
; The LICENSE and the icon are found relative to this file (repo root and crates/tterm-app).

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif
#ifndef SourceExe
  #error Pass /DSourceExe=<path to tterm.exe>
#endif
#ifndef OutputDir
  #define OutputDir "Output"
#endif

; VersionInfoVersion must be numeric: keep X.Y.Z, drop any "-prerelease", append ".0".
#define NumericVersion Copy(AppVersion, 1, Pos("-", AppVersion + "-") - 1)

#define AppName "Tanis Terminal"
#define AppExe "tterm.exe"
#define AppId "codes.tanis.tterm"
#define Publisher "Estanislao Pérez Nartallo"
#define AppURL "https://github.com/tanisperez/terminal"

[Setup]
; Never change this GUID, it identifies the installed app for upgrades and the uninstaller.
; The doubled brace is Inno's escape for a literal one.
AppId={{D4AB7173-D8C0-4C7B-ACCC-F70FC25B7501}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}
AppUpdatesURL={#AppURL}
VersionInfoVersion={#NumericVersion}.0
VersionInfoProductVersion={#NumericVersion}.0
VersionInfoCompany={#Publisher}
VersionInfoDescription={#AppName} setup
VersionInfoProductName={#AppName}
; Per user install, no UAC. With PrivilegesRequired=lowest, {autopf} resolves to
; {localappdata}\Programs, so it is the same folder as writing it out; explicit is clearer.
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\tterm
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; The PATH task changes the user environment: tell running programs (new terminals) about it.
ChangesEnvironment=yes
; The updater runs the installer silently while Tanis Terminal may be open: close it through the
; Restart Manager, never ask for a reboot, and relaunch from [Run].
CloseApplications=yes
RestartApplications=no
SetupIconFile=..\..\crates\tterm-app\assets\tterm.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir={#OutputDir}
OutputBaseFilename=tterm-{#AppVersion}-windows-x86_64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

; The languages of Tanis Terminal itself.
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[CustomMessages]
english.AddToPath=Add tterm to the PATH (run "tterm" or "tterm folder" from a shell)
spanish.AddToPath=Añadir tterm al PATH (ejecutar "tterm" o "tterm carpeta" desde un shell)
english.ContextMenu=Add "Open in Tanis Terminal" to the Explorer menu of folders
spanish.ContextMenu=Añadir "Abrir en Tanis Terminal" al menú del Explorador en carpetas
english.OpenHere=Open in Tanis Terminal
spanish.OpenHere=Abrir en Tanis Terminal
english.Integration=Integration:
spanish.Integration=Integración:

[Tasks]
Name: "addtopath"; Description: "{cm:AddToPath}"; GroupDescription: "{cm:Integration}"
Name: "contextmenu"; Description: "{cm:ContextMenu}"; GroupDescription: "{cm:Integration}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion

; The app sets the explicit AppUserModelID codes.tanis.tterm, so the shortcuts must carry the same
; one or taskbar pinning and grouping break.
[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "{#AppId}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "{#AppId}"; Tasks: desktopicon

; HKA is HKCU here (lowest privileges), so nothing needs admin.
[Registry]
; The user PATH, only when tterm is not on it yet; [Code] removes it on uninstall.
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath(ExpandConstant('{app}'))
; "Open in Tanis Terminal" on folders and on the background of an open folder: a new window
; whose first tab starts there.
Root: HKA; Subkey: "Software\Classes\Directory\shell\tterm"; ValueType: string; ValueName: ""; ValueData: "{cm:OpenHere}"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\tterm"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#AppExe},0"; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\shell\tterm\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%V"""; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\tterm"; ValueType: string; ValueName: ""; ValueData: "{cm:OpenHere}"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\tterm"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#AppExe},0"; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\tterm\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%V"""; Tasks: contextmenu

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\{#AppExe}"; Flags: nowait; Check: RelaunchRequested

[Code]
function RelaunchRequested: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:relaunch|0}') = '1');
end;

const
  EnvironmentKey = 'Environment';

{ Whether Dir is missing from the user PATH, compared without case and trailing backslash. }
function NeedsAddPath(Dir: string): Boolean;
var
  Path: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Path) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(RemoveBackslashUnlessRoot(Dir)) + ';',
    ';' + Uppercase(Path) + ';') = 0;
end;

{ Removes the install folder from the user PATH when Tanis Terminal is uninstalled. }
procedure RemovePath(Dir: string);
var
  Path, Upper, Entry: string;
  Index: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Path) then
    exit;
  { Upper is Path with a ';' in front, so a match at Index there starts at Index - 1 in Path:
    the ';' before the folder, or nothing when the folder is the first entry. }
  Entry := ';' + Uppercase(RemoveBackslashUnlessRoot(Dir));
  Upper := ';' + Uppercase(Path);
  Index := Pos(Entry + ';', Upper + ';');
  if Index = 0 then
    exit;
  if Index > 1 then
    Delete(Path, Index - 1, Length(Entry))
  else
  begin
    { The first entry: the folder without a ';' before it, then the ';' after it, if any. }
    Delete(Path, 1, Length(Entry) - 1);
    if (Length(Path) > 0) and (Path[1] = ';') then
      Delete(Path, 1, 1);
  end;
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Path);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemovePath(ExpandConstant('{app}'));
end;
