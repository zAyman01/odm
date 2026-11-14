; Builds the Windows installer. Run from the repository root:
;   iscc /DAppVersion=0.1.0 /DStageDir=<folder with the binaries> /DOutputName=<file name without .exe> installer\windows\odm.iss
; The extension must already be unpacked at dist\chrome and dist\firefox.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef StageDir
  #define StageDir "..\..\staging"
#endif
#ifndef OutputName
  #define OutputName "odm-setup"
#endif

[Setup]
AppId={{6D1B4E0A-3C52-4E8B-9F0D-7A21C5B8E4F3}
AppName=odm
AppVersion={#AppVersion}
AppPublisher=zAyman01
AppPublisherURL=https://github.com/zAyman01/odm
DefaultDirName={autopf}\odm
DefaultGroupName=odm
UninstallDisplayIcon={app}\odm-gui.exe
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
LicenseFile=..\..\LICENSE-MIT
OutputDir=..\..\release-pkg
OutputBaseFilename={#OutputName}
CloseApplications=force

[Tasks]
Name: "addpath"; Description: "Add odm to PATH, so the ""odm"" command works in a terminal"
Name: "daemonstartup"; Description: "Start the odm download service when I sign in"
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#StageDir}\odm.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\odm-daemon.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\odm-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\dist\chrome\*"; DestDir: "{app}\extension\chrome"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\dist\firefox\*"; DestDir: "{app}\extension\firefox"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\EXTENSION-INSTALL.txt"; DestDir: "{app}\extension"; DestName: "INSTALL.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\odm"; Filename: "{app}\odm-gui.exe"
Name: "{group}\Add the browser extension"; Filename: "{app}\extension\INSTALL.txt"
Name: "{group}\Uninstall odm"; Filename: "{uninstallexe}"
Name: "{userdesktop}\odm"; Filename: "{app}\odm-gui.exe"; Tasks: desktopicon
Name: "{userstartup}\odm download service"; Filename: "{app}\odm-daemon.exe"; Flags: runminimized; Tasks: daemonstartup

[Run]
Filename: "{app}\odm-daemon.exe"; Description: "Start the odm download service now"; Flags: nowait runminimized postinstall skipifsilent
Filename: "{app}\odm-gui.exe"; Description: "Open odm"; Flags: nowait postinstall skipifsilent
Filename: "{app}\extension\INSTALL.txt"; Description: "Show how to add the browser extension"; Flags: shellexec postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM odm-gui.exe /IM odm-daemon.exe"; Flags: runhidden; RunOnceId: "StopOdm"

[Code]
const
  EnvironmentKey = 'Environment';

function PathContains(const Value: string): Boolean;
var
  Current: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Current) then
    Current := '';
  Result := Pos(';' + Lowercase(Value) + ';', ';' + Lowercase(Current) + ';') > 0;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Current: string;
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addpath') then
  begin
    if not PathContains(ExpandConstant('{app}')) then
    begin
      if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Current) then
        Current := '';
      if (Current <> '') and (Copy(Current, Length(Current), 1) <> ';') then
        Current := Current + ';';
      RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Current + ExpandConstant('{app}'));
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Current, Target: string;
  P: Integer;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    if RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Current) then
    begin
      Target := ExpandConstant('{app}');
      P := Pos(';' + Lowercase(Target) + ';', ';' + Lowercase(Current) + ';');
      if P > 0 then
      begin
        Delete(Current, P, Length(Target) + 1);
        if (Current <> '') and (Copy(Current, Length(Current), 1) = ';') then
          Delete(Current, Length(Current), 1);
        RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Current);
      end;
    end;
  end;
end;
