; Inno Setup script for Phosphor's Windows installer.
;
;   iscc /DAppVersion=0.1.7 /DOutputName=phosphor-v0.1.7-windows-x64-setup packaging\windows\phosphor.iss
;
; Installs for the current user without asking for administrator rights,
; or for everyone if chosen on the first page. Registers phosphor.exe for
; telnet://, telnets:// and rlogin:// links, both directly and as a
; "Default apps" choice.

#ifndef AppVersion
  #error Pass the version: /DAppVersion=x.y.z
#endif
#ifndef OutputName
  #define OutputName "phosphor-setup"
#endif
#define Root "..\.."

[Setup]
; Never change AppId: it is how upgrades find an existing installation.
AppId={{337983C0-1ED8-41C5-90E1-A7F2FAB9E709}
AppName=Phosphor
AppVersion={#AppVersion}
AppVerName=Phosphor {#AppVersion}
AppPublisher=The Phosphor contributors
AppPublisherURL=https://badcluster.dev/phosphor/
AppSupportURL=https://git.badcluster.dev/arelas/phosphor/issues
AppUpdatesURL=https://git.badcluster.dev/arelas/phosphor/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\Phosphor
DisableProgramGroupPage=yes
LicenseFile={#Root}\LICENSE
OutputDir={#Root}\dist
OutputBaseFilename={#OutputName}
SetupIconFile={#Root}\crates\phosphor\assets\icon\phosphor.ico
UninstallDisplayIcon={app}\phosphor.exe
UninstallDisplayName=Phosphor
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
; Upgrading closes a running Phosphor rather than asking for a reboot.
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "links"; Description: "Open telnet://, telnets:// and rlogin:// links with Phosphor"

[Files]
Source: "{#Root}\target\release\phosphor.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Phosphor"; Filename: "{app}\phosphor.exe"
Name: "{autodesktop}\Phosphor"; Filename: "{app}\phosphor.exe"; Tasks: desktopicon

[Registry]
; The link handler, as a ProgID that "Default apps" can offer.
Root: HKA; Subkey: "Software\Classes\Phosphor.URL"; ValueType: string; ValueName: ""; ValueData: "BBS link"; Flags: uninsdeletekey; Tasks: links
Root: HKA; Subkey: "Software\Classes\Phosphor.URL"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKA; Subkey: "Software\Classes\Phosphor.URL\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\phosphor.exe"",0"; Tasks: links
Root: HKA; Subkey: "Software\Classes\Phosphor.URL\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\phosphor.exe"" ""%1"""; Tasks: links
Root: HKA; Subkey: "Software\Phosphor\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "Phosphor"; Flags: uninsdeletekey; Tasks: links
Root: HKA; Subkey: "Software\Phosphor\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "ANSI-BBS client for telnet, SSH and RLogin"; Tasks: links
Root: HKA; Subkey: "Software\Phosphor\Capabilities\URLAssociations"; ValueType: string; ValueName: "telnet"; ValueData: "Phosphor.URL"; Tasks: links
Root: HKA; Subkey: "Software\Phosphor\Capabilities\URLAssociations"; ValueType: string; ValueName: "telnets"; ValueData: "Phosphor.URL"; Tasks: links
Root: HKA; Subkey: "Software\Phosphor\Capabilities\URLAssociations"; ValueType: string; ValueName: "rlogin"; ValueData: "Phosphor.URL"; Tasks: links
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "Phosphor"; ValueData: "Software\Phosphor\Capabilities"; Flags: uninsdeletevalue; Tasks: links
; The schemes themselves, for systems where no other program claims them.
; Uninstalling removes only the command it set.
Root: HKA; Subkey: "Software\Classes\telnet"; ValueType: string; ValueName: ""; ValueData: "URL:Telnet Protocol"; Flags: uninsdeletekeyifempty; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\phosphor.exe"" ""%1"""; Flags: uninsdeletekey; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnets"; ValueType: string; ValueName: ""; ValueData: "URL:Telnet over TLS Protocol"; Flags: uninsdeletekeyifempty; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnets"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKA; Subkey: "Software\Classes\telnets\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\phosphor.exe"" ""%1"""; Flags: uninsdeletekey; Tasks: links
Root: HKA; Subkey: "Software\Classes\rlogin"; ValueType: string; ValueName: ""; ValueData: "URL:RLogin Protocol"; Flags: uninsdeletekeyifempty; Tasks: links
Root: HKA; Subkey: "Software\Classes\rlogin"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKA; Subkey: "Software\Classes\rlogin\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\phosphor.exe"" ""%1"""; Flags: uninsdeletekey; Tasks: links

[Run]
Filename: "{app}\phosphor.exe"; Description: "{cm:LaunchProgram,Phosphor}"; Flags: nowait postinstall skipifsilent
