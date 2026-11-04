; Inno Setup script for BadTerm's Windows installer.
;
;   iscc /DAppVersion=0.1.7 /DOutputName=badterm-v0.1.7-windows-x64-setup packaging\windows\badterm.iss
;
; Installs for the current user without asking for administrator rights,
; or for everyone if chosen on the first page. Registers badterm.exe for
; telnet://, telnets:// and rlogin:// links, both directly and as a
; "Default apps" choice.

#ifndef AppVersion
  #error Pass the version: /DAppVersion=x.y.z
#endif
#ifndef OutputName
  #define OutputName "badterm-setup"
#endif
#define Root "..\.."

[Setup]
; Never change AppId: it is how upgrades find an existing installation.
AppId={{337983C0-1ED8-41C5-90E1-A7F2FAB9E709}
AppName=BadTerm
AppVersion={#AppVersion}
AppVerName=BadTerm {#AppVersion}
AppPublisher=Bad Cluster
AppCopyright=Copyright (C) 2026 Bad Cluster
AppPublisherURL=https://badterm.com/
AppSupportURL=https://git.badcluster.dev/arelas/badterm/issues
AppUpdatesURL=https://git.badcluster.dev/arelas/badterm/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\BadTerm
DisableProgramGroupPage=yes
LicenseFile={#Root}\LICENSE
OutputDir={#Root}\dist
OutputBaseFilename={#OutputName}
SetupIconFile={#Root}\crates\badterm\assets\icon\badterm.ico
UninstallDisplayIcon={app}\badterm.exe
UninstallDisplayName=BadTerm
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
; Upgrading closes a running BadTerm rather than asking for a reboot.
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "links"; Description: "Open telnet://, telnets:// and rlogin:// links with BadTerm"

[Files]
Source: "{#Root}\target\release\badterm.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\BadTerm"; Filename: "{app}\badterm.exe"
Name: "{autodesktop}\BadTerm"; Filename: "{app}\badterm.exe"; Tasks: desktopicon

[Registry]
; The link handler, as a ProgID that "Default apps" can offer.
Root: HKA; Subkey: "Software\Classes\BadTerm.URL"; ValueType: string; ValueName: ""; ValueData: "BBS link"; Flags: uninsdeletekey; Tasks: links
Root: HKA; Subkey: "Software\Classes\BadTerm.URL"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKA; Subkey: "Software\Classes\BadTerm.URL\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\badterm.exe"",0"; Tasks: links
Root: HKA; Subkey: "Software\Classes\BadTerm.URL\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\badterm.exe"" ""%1"""; Tasks: links
Root: HKA; Subkey: "Software\BadTerm\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "BadTerm"; Flags: uninsdeletekey; Tasks: links
Root: HKA; Subkey: "Software\BadTerm\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "ANSI-BBS client for telnet, SSH and RLogin"; Tasks: links
Root: HKA; Subkey: "Software\BadTerm\Capabilities\URLAssociations"; ValueType: string; ValueName: "telnet"; ValueData: "BadTerm.URL"; Tasks: links
Root: HKA; Subkey: "Software\BadTerm\Capabilities\URLAssociations"; ValueType: string; ValueName: "telnets"; ValueData: "BadTerm.URL"; Tasks: links
Root: HKA; Subkey: "Software\BadTerm\Capabilities\URLAssociations"; ValueType: string; ValueName: "rlogin"; ValueData: "BadTerm.URL"; Tasks: links
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "BadTerm"; ValueData: "Software\BadTerm\Capabilities"; Flags: uninsdeletevalue; Tasks: links
; The schemes themselves, for systems where no other program claims them.
; Uninstalling removes only the command it set.
Root: HKA; Subkey: "Software\Classes\telnet"; ValueType: string; ValueName: ""; ValueData: "URL:Telnet Protocol"; Flags: uninsdeletekeyifempty; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\badterm.exe"" ""%1"""; Flags: uninsdeletekey; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnets"; ValueType: string; ValueName: ""; ValueData: "URL:Telnet over TLS Protocol"; Flags: uninsdeletekeyifempty; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnets"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnets\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\badterm.exe"" ""%1"""; Flags: uninsdeletekey; Tasks: links
Root: HKA; Subkey: "Software\Classes\rlogin"; ValueType: string; ValueName: ""; ValueData: "URL:RLogin Protocol"; Flags: uninsdeletekeyifempty; Tasks: links
Root: HKA; Subkey: "Software\Classes\rlogin"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKA; Subkey: "Software\Classes\rlogin\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\badterm.exe"" ""%1"""; Flags: uninsdeletekey; Tasks: links

[Run]
Filename: "{app}\badterm.exe"; Description: "{cm:LaunchProgram,BadTerm}"; Flags: nowait postinstall skipifsilent
