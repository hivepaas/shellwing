; shellwing's installer for Windows (NSIS 3, Unicode), made by
; scripts/package_windows.sh:
;
;   makensis /DVERSION=0.1.0 /DFILEVERSION=0.1.0.0 /DEXE=path\to\shellwing.exe /DOUT=setup.exe installer.nsi
;
; FILEVERSION is VERSION as Windows writes a file's version: four numbers, no
; beta (0.3.0-beta2 is 0.3.0.0).
;
; It installs for the user, with no administrator rights, into
; %LOCALAPPDATA%\Programs\shellwing: the place the updater knows as the
; installer's (crates/update/src/install.rs), and runs the next one quietly
; (/S) over it. The settings, the vault and the hosts are the user's, under
; %APPDATA%: neither installing nor uninstalling touches them.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!ifndef VERSION
  !error "VERSION is not defined"
!endif
!ifndef EXE
  !error "EXE is not defined"
!endif
!ifndef OUT
  !error "OUT is not defined"
!endif
!ifndef FILEVERSION
  !error "FILEVERSION is not defined"
!endif

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\shellwing"

Name "shellwing"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\shellwing"
RequestExecutionLevel user
BrandingText "shellwing ${VERSION}"

VIProductVersion "${FILEVERSION}"
VIAddVersionKey "ProductName" "shellwing"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "shellwing installer"
VIAddVersionKey "CompanyName" "HivePaaS"
VIAddVersionKey "LegalCopyright" "Copyright (C) 2026 HivePaaS"

!define MUI_ICON "..\icons\shellwing.ico"
!define MUI_UNICON "..\icons\shellwing.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\shellwing.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start shellwing"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "shellwing"
  ; The app may still be closing (the updater starts this as it quits): the
  ; new one is unpacked aside, then copied in, tried again for five seconds,
  ; then asked about (Retry / Cancel; Cancel when quiet).
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "/oname=shellwing.exe" "${EXE}"
  SetOutPath "$INSTDIR"
  StrCpy $0 0
  retry:
    ClearErrors
    CopyFiles /SILENT "$PLUGINSDIR\shellwing.exe" "$INSTDIR\shellwing.exe"
    IfErrors 0 copied
    IntOp $0 $0 + 1
    IntCmp $0 10 ask
    Sleep 500
    Goto retry
  ask:
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "shellwing is still running. Quit it, then choose Retry." /SD IDCANCEL IDRETRY retry
    Abort
  copied:
  File "/oname=LICENSE.txt" "..\..\LICENSE"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortcut "$SMPROGRAMS\shellwing.lnk" "$INSTDIR\shellwing.exe"

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "shellwing"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "HivePaaS"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://shellwing.app"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\shellwing.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\shellwing.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\shellwing.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
