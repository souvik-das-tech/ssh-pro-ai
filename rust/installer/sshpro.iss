; Windows installer for the Rust build of SSH Pro AI (Inno Setup 6).
; Build it with rust\installer.cmd, which passes AppVersion from Cargo.toml.
;
; Installs per user by default (no admin needed) into %LOCALAPPDATA%\Programs\SSH Pro AI, adds
; Start menu and desktop shortcuts and an entry in Settings > Apps. Uninstalling removes the app
; but keeps the user's data (%LOCALAPPDATA%\SSH-Pro-AI: servers, keys, chats).

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#define AppName "SSH Pro AI"
#define AppExe "sshpro.exe"

[Setup]
AppId={{6B3E2F4A-9C1D-4E7B-A5F2-3D8C1E0B7A94}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=SSH Pro AI
AppPublisherURL=https://github.com/souvik-das-tech/ssh-pro-ai
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\target\installer
OutputBaseFilename=SSH-Pro-AI-Setup-{#AppVersion}
SetupIconFile=..\..\sshpro.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/ultra64
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\WebView2Loader.dll"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; same AppUserModelID as the app sets, so a pinned taskbar icon and the window group together
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "SSHProAI.Desktop"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "SSHProAI.Desktop"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
const
  WebView2Id = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  WebView2Bootstrapper = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703';

var
  DownloadPage: TDownloadWizardPage;

function HasWebView2Key(Root: Integer; Key: String): Boolean;
var
  Version: String;
begin
  Result := RegQueryStringValue(Root, Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0');
end;

{ The window is drawn by the Microsoft Edge WebView2 Runtime (built into Windows 11). }
function WebView2Installed: Boolean;
begin
  Result := HasWebView2Key(HKLM, 'SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\' + WebView2Id)
    or HasWebView2Key(HKLM, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\' + WebView2Id)
    or HasWebView2Key(HKCU, 'Software\Microsoft\EdgeUpdate\Clients\' + WebView2Id);
end;

procedure InitializeWizard;
begin
  DownloadPage := CreateDownloadPage('Microsoft Edge WebView2 Runtime',
    'SSH Pro AI needs the WebView2 Runtime to show its window. Downloading it from Microsoft...', nil);
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  ResultCode: Integer;
begin
  Result := True;
  if (CurPageID = wpReady) and not WebView2Installed then
  begin
    DownloadPage.Clear;
    DownloadPage.Add(WebView2Bootstrapper, 'MicrosoftEdgeWebview2Setup.exe', '');
    DownloadPage.Show;
    try
      try
        DownloadPage.Download;
        Exec(ExpandConstant('{tmp}\MicrosoftEdgeWebview2Setup.exe'), '/silent /install', '',
             SW_HIDE, ewWaitUntilTerminated, ResultCode);
      except
        MsgBox('The Microsoft Edge WebView2 Runtime could not be installed automatically.' + #13#10 +
               'SSH Pro AI will be installed; if its window stays empty, install WebView2 from' + #13#10 +
               'https://developer.microsoft.com/microsoft-edge/webview2/', mbInformation, MB_OK);
      end;
    finally
      DownloadPage.Hide;
    end;
  end;
end;
