; BoogieBox Server Windows Installer
; Built automatically by build-server-rust.bat (calls iscc with /DReleaseDir=<abs-path>).
; Manual build from repo root:
;   iscc "/DAppVersion=x.x.x" "/DReleaseDir=D:\path\to\Releases\boogiebox-x.x.x-win-rs" installer\boogiebox-server.iss

#ifndef AppVersion
  #define AppVersion "0.7.117"
#endif
#ifndef ReleaseDir
  #define ReleaseDir "..\Releases\boogiebox-" + AppVersion + "-win-rs"
#endif

[Setup]
AppName=BoogieBox Server
AppVersion={#AppVersion}
AppPublisher=BoogieBox
DefaultDirName={autopf}\BoogieBox
DefaultGroupName=BoogieBox
AllowNoIcons=yes
OutputDir=..\Releases
OutputBaseFilename=boogiebox-{#AppVersion}-win-setup
SetupIconFile=..\desktop\src-tauri\icons\icon.ico
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
; Installs machine-wide to Program Files and prompts for elevation.
PrivilegesRequired=admin
DisableProgramGroupPage=yes
UninstallDisplayName=BoogieBox Server {#AppVersion}
UninstallDisplayIcon={app}\boogiebox-server.exe

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional icons:"; Flags: unchecked
Name: "serviceinstall"; Description: "Install as a Windows &service"; GroupDescription: "Server startup:"; Flags: unchecked
Name: "boogiemix"; Description: "[HIGHLY EXPERIMENTAL - still in progress] Install BoogieMix &deep analysis (AI stem separation - requires internet, large download, 2-5 GB installed, 10-30 min)"; GroupDescription: "Optional features:"; Flags: unchecked

[Dirs]
Name: "{commonappdata}\BoogieBox\logs"; Permissions: users-modify

[Files]
Source: "{#ReleaseDir}\boogiebox-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\boogiebox-service.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\boogiebox-service.xml"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\start.bat"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\client\build\*"; DestDir: "{app}\client\build"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#ReleaseDir}\resources\*"; DestDir: "{app}\resources"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; Vite asset names are content-hashed, so remove the prior packaged client before copying the new build.
Type: filesandordirs; Name: "{app}\client\build"
; Clean up Node.js runtime artifacts left by older Node SEA installs.
Type: filesandordirs; Name: "{app}\node_modules"

[Icons]
Name: "{group}\BoogieBox Server"; Filename: "{app}\boogiebox-server.exe"; WorkingDir: "{app}"; Comment: "Start the BoogieBox media server"
Name: "{group}\Uninstall BoogieBox Server"; Filename: "{uninstallexe}"
Name: "{commondesktop}\BoogieBox Server"; Filename: "{app}\boogiebox-server.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\boogiebox-server.exe"; Description: "Start BoogieBox Server now (open http://localhost:3001 in your browser)"; Flags: nowait postinstall skipifsilent; WorkingDir: "{app}"; Tasks: not serviceinstall

[Code]
var
  ServiceAccountModePage: TInputOptionWizardPage;
  ExistingServiceAccountPage: TInputQueryWizardPage;
  BoogieMixInfoPage: TOutputMsgWizardPage;

function CoCreateGuid(var Guid: TGUID): Integer;
external 'CoCreateGuid@ole32.dll stdcall';

function StringFromGUID2(var Guid: TGUID; GuidString: String; GuidStringMax: Integer): Integer;
external 'StringFromGUID2@ole32.dll stdcall';

function RunFirewallCommand(Params: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := Exec(ExpandConstant('{sys}\netsh.exe'), Params, '', SW_HIDE, ewWaitUntilTerminated, ResultCode) and (ResultCode = 0);
  Log('Firewall command exited with code ' + IntToStr(ResultCode) + ': netsh ' + Params);
end;

function EscapePowerShellSingleQuoted(Value: String): String;
begin
  StringChangeEx(Value, '''', '''''', True);
  Result := Value;
end;

function RunPowerShellScript(ScriptPath: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := Exec(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + ScriptPath + '"',
    '',
    SW_HIDE,
    ewWaitUntilTerminated,
    ResultCode
  ) and (ResultCode = 0);
  Log('PowerShell service helper exited with code ' + IntToStr(ResultCode) + '.');
end;

function RunServiceCommand(Params: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := Exec(
    ExpandConstant('{app}\boogiebox-service.exe'),
    Params,
    ExpandConstant('{app}'),
    SW_HIDE,
    ewWaitUntilTerminated,
    ResultCode
  ) and (ResultCode = 0);
end;

function NewGuidString(): String;
var
  Guid: TGUID;
  GuidString: String;
begin
  if CoCreateGuid(Guid) <> 0 then
    RaiseException('Could not generate a BoogieBox service account password.');
  SetLength(GuidString, 39);
  if StringFromGUID2(Guid, GuidString, 39) = 0 then
    RaiseException('Could not format a BoogieBox service account password.');
  Result := GuidString;
end;

function GenerateServicePassword(): String;
begin
  Result := 'Bb1!' + NewGuidString() + NewGuidString() + 'Zz9#';
end;

function RunServiceAccessHelper(ScriptBody: String): Boolean;
var
  Script: String;
  DiagnosticsPath: String;
  DiagnosticsDir: String;
  ScriptPath: String;
begin
  DiagnosticsDir := ExpandConstant('{commonappdata}\BoogieBox');
  DiagnosticsPath := DiagnosticsDir + '\installer-service.log';
  ScriptPath := ExpandConstant('{tmp}\boogiebox-service-account.ps1');
  Log('Writing BoogieBox service installer diagnostics to ' + DiagnosticsPath + '.');
  if not ForceDirectories(DiagnosticsDir) then
  begin
    Log('Could not create diagnostics directory ' + DiagnosticsDir + '.');
    Result := False;
    exit;
  end;
  SaveStringToFile(
    DiagnosticsPath,
    '[' + GetDateTimeString('yyyy-mm-dd hh:nn:ss', '-', ':') + '] Installer started service account helper.'#13#10,
    False
  );
  Script :=
    '$ErrorActionPreference = ''Stop''; ' +
    '$diagnostics = ''' + DiagnosticsPath + '''; ' +
    'New-Item -ItemType Directory -Force -Path (Split-Path -Parent $diagnostics) | Out-Null; ' +
    'Set-Content -Path $diagnostics -Value (''['' + (Get-Date -Format o) + ''] BoogieBox service account setup started.''); ' +
    'function Add-Diagnostic([string] $message) { Add-Content -Path $diagnostics -Value (''['' + (Get-Date -Format o) + ''] '' + $message) }; ' +
    'function Invoke-LoggedNative([string] $step, [scriptblock] $command) { Add-Diagnostic ($step + '' started.''); $previousErrorAction = $ErrorActionPreference; $ErrorActionPreference = ''Continue''; try { $output = & $command 2>&1; $exit = $LASTEXITCODE } finally { $ErrorActionPreference = $previousErrorAction }; Add-Diagnostic ($step + '' exit code: '' + $exit); foreach ($line in @($output)) { if ($null -ne $line -and $line.ToString().Trim() -ne '''') { Add-Diagnostic ($step + '': '' + $line.ToString()) } }; if ($exit -ne 0) { throw ($step + '' failed with exit code '' + $exit + ''.'') } }; ' +
    'try { ' + ScriptBody +
    'Add-Diagnostic ''BoogieBox service account setup completed.'' } catch { Add-Diagnostic (''ERROR: '' + $_.Exception.Message); Add-Diagnostic (''ERROR detail: '' + ($_ | Out-String).Trim()); throw }';
  if not SaveStringToFile(ScriptPath, Script, False) then
  begin
    Log('Could not write PowerShell service helper script to ' + ScriptPath + '.');
    Result := False;
    exit;
  end;
  Log('PowerShell service helper script written to ' + ScriptPath + '.');
  Result := RunPowerShellScript(ScriptPath);
  DeleteFile(ScriptPath);
end;

function GrantServiceFoldersBySid(ServiceSidExpression: String): String;
begin
  Result :=
    '$serviceSidGrant = ''*'' + ' + ServiceSidExpression + ' + '':(OI)(CI)M''; ' +
    '$paths = @(''' + ExpandConstant('{commonappdata}\BoogieBox') + ''', ''' + ExpandConstant('{app}\logs') + '''); ' +
    'foreach ($path in $paths) { Add-Diagnostic (''Grant folder access started: '' + $path); New-Item -ItemType Directory -Force -Path $path | Out-Null; Invoke-LoggedNative (''Grant folder access '' + $path) { & $env:SystemRoot\System32\icacls.exe $path ''/grant'' $serviceSidGrant } }; ';
end;

function CreateOrUpdateServiceAccount(Password: String): Boolean;
var
  ScriptBody: String;
  EscapedPassword: String;
begin
  EscapedPassword := EscapePowerShellSingleQuoted(Password);
  ScriptBody :=
    '$password = ''' + EscapedPassword + '''; ' +
    '$securePassword = ConvertTo-SecureString $password -AsPlainText -Force; ' +
    '$user = Get-LocalUser -Name ''BoogieBoxService'' -ErrorAction SilentlyContinue; Add-Diagnostic (''BoogieBoxService account exists: '' + ($null -ne $user)); ' +
    'if ($null -eq $user) { Add-Diagnostic ''Create local user started.''; New-LocalUser -Name ''BoogieBoxService'' -Password $securePassword -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword -Description ''BoogieBox media server service account'' | Out-Null; Add-Diagnostic ''Create local user completed.'' } ' +
    'else { Add-Diagnostic ''Set local user password started.''; Set-LocalUser -Name ''BoogieBoxService'' -Password $securePassword -PasswordNeverExpires $true; Add-Diagnostic ''Set local user password completed.'' }; ' +
    'Add-Diagnostic ''Enable local user started.''; Enable-LocalUser -Name ''BoogieBoxService''; Add-Diagnostic ''Enable local user completed.''; ' +
    '$serviceUser = Get-LocalUser -Name ''BoogieBoxService''; Add-Diagnostic (''BoogieBoxService SID resolved: '' + $serviceUser.SID.Value); ' +
    'Add-Diagnostic ''ADSI metadata update started.''; try { $user = [ADSI](''WinNT://'' + $env:COMPUTERNAME + ''/BoogieBoxService,user''); $user.Put(''Description'', ''BoogieBox media server service account''); $user.Put(''UserFlags'', ([int]$user.Get(''UserFlags'') -bor 0x10000 -bor 0x40) -band (-bnot 0x2)); $user.SetInfo(); Add-Diagnostic ''ADSI metadata update completed.'' } catch { Add-Diagnostic (''ADSI metadata update warning: '' + $_.Exception.Message) }; ' +
    GrantServiceFoldersBySid('$serviceUser.SID.Value');
  Result := RunServiceAccessHelper(ScriptBody);
end;

function GrantExistingServiceAccountAccess(UserName: String): Boolean;
var
  ScriptBody: String;
  EscapedUserName: String;
begin
  EscapedUserName := EscapePowerShellSingleQuoted(UserName);
  ScriptBody :=
    '$serviceAccount = ''' + EscapedUserName + '''; ' +
    'Add-Diagnostic (''Resolve existing service account SID started: '' + $serviceAccount); ' +
    'if ($serviceAccount.StartsWith(''.\'')) { ' +
    '$localUserName = $serviceAccount.Substring(2); if ($localUserName -eq '''') { throw ''Existing local service account name is empty.'' }; ' +
    '$localUser = Get-LocalUser -Name $localUserName -ErrorAction Stop; $serviceSid = $localUser.SID.Value ' +
    '} else { $serviceSid = ([System.Security.Principal.NTAccount] $serviceAccount).Translate([System.Security.Principal.SecurityIdentifier]).Value }; ' +
    'Add-Diagnostic (''Existing service account SID resolved: '' + $serviceSid); ' +
    GrantServiceFoldersBySid('$serviceSid');
  Result := RunServiceAccessHelper(ScriptBody);
end;

function ConfigureServiceLogon(UserName: String; Password: String): Boolean;
var
  ScriptBody: String;
  EscapedUserName: String;
  EscapedPassword: String;
begin
  EscapedUserName := EscapePowerShellSingleQuoted(UserName);
  EscapedPassword := EscapePowerShellSingleQuoted(Password);
  ScriptBody :=
    '$serviceAccount = ''' + EscapedUserName + '''; ' +
    '$password = ''' + EscapedPassword + '''; ' +
    'Add-Diagnostic (''Configure service logon started: '' + $serviceAccount); ' +
    '$service = Get-CimInstance -ClassName Win32_Service -Filter ''Name = "BoogieBoxServer"'' -ErrorAction Stop; ' +
    '$result = Invoke-CimMethod -InputObject $service -MethodName Change -Arguments @{ StartName = $serviceAccount; StartPassword = $password }; ' +
    'Add-Diagnostic (''Configure service logon return code: '' + $result.ReturnValue); ' +
    'if ($result.ReturnValue -ne 0) { throw (''Configure service logon failed with Win32_Service.Change return code '' + $result.ReturnValue + ''.'') }; ';
  Result := RunServiceAccessHelper(ScriptBody);
end;

function InstallBoogieBoxService(UserName: String; Password: String): Boolean;
begin
  RunServiceCommand('stop');
  RunServiceCommand('uninstall');
  Result := RunServiceCommand('install');
  if Result then
    Result := ConfigureServiceLogon(UserName, Password);
  if Result then
    Result := RunServiceCommand('start');
end;

procedure RemoveBoogieBoxService();
begin
  RunServiceCommand('stop');
  RunServiceCommand('uninstall');
end;

procedure InitializeWizard();
begin
  ServiceAccountModePage := CreateInputOptionPage(
    wpSelectTasks,
    'BoogieBox Service Account',
    'Choose the Windows account for the service.',
    'The default local BoogieBox account is good for local folders. Choose an existing account when UNC media shares need Windows or NAS credentials.',
    True,
    False
  );
  ServiceAccountModePage.Add('Create or update local .\BoogieBoxService with an installer-generated password');
  ServiceAccountModePage.Add('Use an existing Windows account for UNC library access');
  ServiceAccountModePage.SelectedValueIndex := 0;

  ExistingServiceAccountPage := CreateInputQueryPage(
    ServiceAccountModePage.ID,
    'Existing Service Account',
    'Enter credentials for the Windows account that should run BoogieBox.',
    'Use DOMAIN\User, COMPUTER\User, or .\User. Grant that account access to UNC shares and external BoogieBox data folders.'
  );
  ExistingServiceAccountPage.Add('Account:', False);
  ExistingServiceAccountPage.Add('Password:', True);
  ExistingServiceAccountPage.Add('Confirm password:', True);

  BoogieMixInfoPage := CreateOutputMsgPage(
    wpSelectTasks,
    'BoogieMix Deep Analysis',
    'AI-powered audio stem separation for enhanced mix transitions',
    'Selecting this option will perform the following after file installation:' + #13#10 +
    '' + #13#10 +
    '  - Install a compatible Python version if not already present (via winget or python.org)' + #13#10 +
    '  - Detect NVIDIA/CUDA and install GPU PyTorch when available' + #13#10 +
    '  - Fall back to CPU PyTorch if CUDA is unavailable or cannot be verified' + #13#10 +
    '  - Download and install Demucs plus the default stem-separation model' + #13#10 +
    '  - Create a managed Python environment inside the BoogieBox install folder' + #13#10 +
    '' + #13#10 +
    'Requirements:' + #13#10 +
    '  - Active internet connection during installation' + #13#10 +
    '  - Approximately 2-5 GB of free disk space' + #13#10 +
    '  - 10-30 minutes depending on download speed and GPU package size' + #13#10 +
    '' + #13#10 +
    'BoogieBox Server installs and works normally regardless of this choice.' + #13#10 +
    'Deep analysis can be repaired or enabled later by running bootstrap_env.ps1 from' + #13#10 +
    'resources\Services\boogiemix\python\ inside the install folder.' + #13#10 +
    '' + #13#10 +
    'Installer diagnostics are written to ProgramData\BoogieBox\installer-boogiemix.log.'
  );
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result :=
    ((PageID = ServiceAccountModePage.ID) and (not WizardIsTaskSelected('serviceinstall'))) or
    ((PageID = ExistingServiceAccountPage.ID) and
      ((not WizardIsTaskSelected('serviceinstall')) or (ServiceAccountModePage.SelectedValueIndex = 0))) or
    ((PageID = BoogieMixInfoPage.ID) and (not WizardIsTaskSelected('boogiemix')));
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = ExistingServiceAccountPage.ID then
  begin
    if ExistingServiceAccountPage.Values[0] = '' then
    begin
      MsgBox('Enter the Windows account that should run the BoogieBox service.', mbError, MB_OK);
      Result := False;
    end
    else if (Pos('"', ExistingServiceAccountPage.Values[0]) > 0) or (Pos('"', ExistingServiceAccountPage.Values[1]) > 0) then
    begin
      MsgBox('The existing service account name and password cannot contain a double quote.', mbError, MB_OK);
      Result := False;
    end
    else if ExistingServiceAccountPage.Values[1] = '' then
    begin
      MsgBox('Enter the existing service account password.', mbError, MB_OK);
      Result := False;
    end
    else if ExistingServiceAccountPage.Values[1] <> ExistingServiceAccountPage.Values[2] then
    begin
      MsgBox('The existing service account passwords do not match.', mbError, MB_OK);
      Result := False;
    end;
  end;
end;

function BuildBoogieMixSetupScript(AppDir: String): String;
var
  LogPath: String;
  PythonDir: String;
begin
  LogPath := ExpandConstant('{commonappdata}') + '\BoogieBox\installer-boogiemix.log';
  PythonDir := AppDir + '\resources\Services\boogiemix\python';
  Result :=
    '$host.UI.RawUI.WindowTitle = ''BoogieBox - Installing BoogieMix (may take 10-30 minutes)''' + #13#10 +
    '$logPath = ''' + LogPath + '''' + #13#10 +
    'New-Item -ItemType Directory -Force -Path (Split-Path $logPath) | Out-Null' + #13#10 +
    'function Write-Log { param([string]$m); $l = "[$(Get-Date -Format ''yyyy-MM-dd HH:mm:ss'')] $m"; Write-Host $l; Add-Content -Path $logPath -Value $l }' + #13#10 +
    'Write-Log ''BoogieMix setup started.''' + #13#10 +
    'Write-Log ''Running bootstrap_env.ps1 (resolves/installs a Python version matching the bundled madmom wheel, auto CUDA/CPU PyTorch, Demucs model download - this may take 10-30 minutes)...''' + #13#10 +
    'try {' + #13#10 +
    '  & ''' + PythonDir + '\bootstrap_env.ps1'' -Auto -PrimeDemucsModel -Force *>&1 | Tee-Object -Append -FilePath $logPath' + #13#10 +
    '} catch {' + #13#10 +
    '  Write-Log "Bootstrap threw an exception: $_"' + #13#10 +
    '}' + #13#10 +
    'Write-Log ''Verifying BoogieMix environment...''' + #13#10 +
    '$venvPy = ''' + PythonDir + '\.venv\Scripts\python.exe''' + #13#10 +
    'if (-not (Test-Path $venvPy)) {' + #13#10 +
    '  Write-Log ''FAILED: .venv python not found. Bootstrap may have failed before creating the environment.''' + #13#10 +
    '  Write-Log "Log saved to: $logPath"' + #13#10 +
    '  Read-Host ''Press Enter to close''' + #13#10 +
    '  exit 1' + #13#10 +
    '}' + #13#10 +
    '& $venvPy -c ''import torch, demucs'' 2>&1 | Out-Null' + #13#10 +
    'if ($LASTEXITCODE -ne 0) {' + #13#10 +
    '  Write-Log ''FAILED: torch/demucs not importable after bootstrap.''' + #13#10 +
    '  Write-Log "Log saved to: $logPath"' + #13#10 +
    '  Read-Host ''Press Enter to close''' + #13#10 +
    '  exit 1' + #13#10 +
    '}' + #13#10 +
    'Write-Log ''BoogieMix setup completed successfully.''';
end;

function RunBoogieMixSetup(AppDir: String): Boolean;
var
  ScriptPath: String;
  ResultCode: Integer;
begin
  ScriptPath := ExpandConstant('{tmp}') + '\bb_boogiemix_setup.ps1';
  if not SaveStringToFile(ScriptPath, BuildBoogieMixSetupScript(AppDir), False) then
  begin
    Log('Could not write BoogieMix setup script to ' + ScriptPath + '.');
    Result := False;
    exit;
  end;
  WizardForm.StatusLabel.Caption := 'Setting up BoogieMix deep analysis (may take 10-30 minutes)...';
  Result := Exec(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -ExecutionPolicy Bypass -File "' + ScriptPath + '"',
    '', SW_SHOW, ewWaitUntilTerminated, ResultCode
  ) and (ResultCode = 0);
  DeleteFile(ScriptPath);
  Log('BoogieMix setup exit code: ' + IntToStr(ResultCode));
end;

procedure RefreshFirewallRule();
var
  AppExe: String;
begin
  AppExe := ExpandConstant('{app}\boogiebox-server.exe');
  RunFirewallCommand('advfirewall firewall delete rule name="BoogieBox Server"');
  if not RunFirewallCommand('advfirewall firewall add rule name="BoogieBox Server" dir=in action=allow program="' + AppExe + '" enable=yes protocol=TCP profile=any') then
    Log('Could not create Windows Firewall allow rule for ' + AppExe + '.');
end;

procedure RemoveFirewallRule();
var
  AppExe: String;
begin
  AppExe := ExpandConstant('{app}\boogiebox-server.exe');
  RunFirewallCommand('advfirewall firewall delete rule name="BoogieBox Server" program="' + AppExe + '"');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ServicePassword: String;
  ServiceUserName: String;
begin
  if CurStep = ssPostInstall then
  begin
    RefreshFirewallRule();
    if WizardIsTaskSelected('serviceinstall') then
    begin
      if ServiceAccountModePage.SelectedValueIndex = 0 then
      begin
        ServiceUserName := '.\BoogieBoxService';
        ServicePassword := GenerateServicePassword();
        if not CreateOrUpdateServiceAccount(ServicePassword) then
          RaiseException('Could not create or update .\BoogieBoxService. See ' + ExpandConstant('{commonappdata}\BoogieBox\installer-service.log') + '.');
      end
      else
      begin
        ServiceUserName := ExistingServiceAccountPage.Values[0];
        ServicePassword := ExistingServiceAccountPage.Values[1];
        if not GrantExistingServiceAccountAccess(ServiceUserName) then
          RaiseException('Could not grant the existing service account BoogieBox folder access. See ' + ExpandConstant('{commonappdata}\BoogieBox\installer-service.log') + '.');
      end;
      if not InstallBoogieBoxService(ServiceUserName, ServicePassword) then
        RaiseException('Could not install the BoogieBox Windows service.');
    end;
    if WizardIsTaskSelected('boogiemix') then
    begin
      if not RunBoogieMixSetup(ExpandConstant('{app}')) then
        MsgBox(
          'BoogieMix deep analysis dependencies could not be installed.' + #13#10 +
          '' + #13#10 +
          'BoogieBox Server is installed and will work normally without deep analysis.' + #13#10 +
          'To enable deep analysis later, ensure Python 3.10+ is installed and run:' + #13#10 +
          '' + #13#10 +
          ExpandConstant('{app}') + '\resources\Services\boogiemix\python\bootstrap_env.ps1' + #13#10 +
          '' + #13#10 +
          'See ' + ExpandConstant('{commonappdata}') + '\BoogieBox\installer-boogiemix.log for details.',
          mbInformation, MB_OK
        );
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    RemoveBoogieBoxService();
    RemoveFirewallRule();
  end;
end;
