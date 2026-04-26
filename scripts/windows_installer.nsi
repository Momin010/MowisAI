; ---------------------------------------------------------------------------
; MowisAI NSIS installer script
; Produces: dist\MowisAI-Setup.exe
;
; Requirements:
;   NSIS 3.x  https://nsis.sourceforge.io
;   MUI2.nsh  (bundled with NSIS 3)
;
; Build:
;   makensis scripts\windows_installer.nsi
; ---------------------------------------------------------------------------

Unicode true

; ---------------------------------------------------------------------------
; Application metadata
; ---------------------------------------------------------------------------

!define APP_NAME      "MowisAI"
!define APP_VERSION   "0.1.0"
!define APP_PUBLISHER "MowisAI"
!define APP_URL       "https://mowisai.com"
!define APP_EXE       "mowisai.exe"

!define UNINSTALL_KEY \
  "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

; ---------------------------------------------------------------------------
; Installer settings
; ---------------------------------------------------------------------------

Name              "${APP_NAME} ${APP_VERSION}"
OutFile           "dist\MowisAI-Setup.exe"
InstallDir        "$PROGRAMFILES64\${APP_NAME}"
InstallDirRegKey  HKLM "Software\${APP_NAME}" ""
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText      "${APP_NAME} ${APP_VERSION} installer"

; ---------------------------------------------------------------------------
; Modern UI 2
; ---------------------------------------------------------------------------

!include "MUI2.nsh"

; Pages — install
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN          "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT     "Launch ${APP_NAME} now"
!define MUI_FINISHPAGE_LINK         "Visit ${APP_URL}"
!define MUI_FINISHPAGE_LINK_LOCATION "${APP_URL}"
!insertmacro MUI_PAGE_FINISH

; Pages — uninstall
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; ---------------------------------------------------------------------------
; Version info embedded in the exe
; ---------------------------------------------------------------------------

VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey "ProductName"      "${APP_NAME}"
VIAddVersionKey "ProductVersion"   "${APP_VERSION}"
VIAddVersionKey "CompanyName"      "${APP_PUBLISHER}"
VIAddVersionKey "LegalCopyright"   "Copyright (C) 2026 ${APP_PUBLISHER}"
VIAddVersionKey "FileDescription"  "${APP_NAME} Installer"
VIAddVersionKey "FileVersion"      "${APP_VERSION}"

; ---------------------------------------------------------------------------
; Install section
; ---------------------------------------------------------------------------

Section "Core (required)" SecCore

  SectionIn RO   ; cannot be deselected

  SetOutPath "$INSTDIR"

  ; Main binary
  File "dist\mowisai.exe"

  ; WSL2 helper script (used at first-run, not auto-executed by installer)
  File "scripts\check_wsl2.ps1"

  ; Write uninstaller
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; ---------------------------------------------------------------------------
  ; Shortcuts
  ; ---------------------------------------------------------------------------

  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut  "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" \
                  "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

  CreateShortcut  "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk" \
                  "$INSTDIR\Uninstall.exe"

  CreateShortcut  "$DESKTOP\${APP_NAME}.lnk" \
                  "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

  ; ---------------------------------------------------------------------------
  ; Registry — installation path + Add/Remove Programs entry
  ; ---------------------------------------------------------------------------

  WriteRegStr HKLM "Software\${APP_NAME}" "" "$INSTDIR"

  WriteRegStr   HKLM "${UNINSTALL_KEY}" "DisplayName"          "${APP_NAME}"
  WriteRegStr   HKLM "${UNINSTALL_KEY}" "DisplayVersion"       "${APP_VERSION}"
  WriteRegStr   HKLM "${UNINSTALL_KEY}" "Publisher"            "${APP_PUBLISHER}"
  WriteRegStr   HKLM "${UNINSTALL_KEY}" "URLInfoAbout"         "${APP_URL}"
  WriteRegStr   HKLM "${UNINSTALL_KEY}" "InstallLocation"      "$INSTDIR"
  WriteRegStr   HKLM "${UNINSTALL_KEY}" "UninstallString"      \
                  '"$INSTDIR\Uninstall.exe"'
  WriteRegStr   HKLM "${UNINSTALL_KEY}" "QuietUninstallString" \
                  '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify"             1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair"             1

  ; Estimated size (KB)
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "EstimatedSize" $0

SectionEnd

; ---------------------------------------------------------------------------
; Uninstall section
; ---------------------------------------------------------------------------

Section "Uninstall"

  ; Remove files
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\check_wsl2.ps1"
  Delete "$INSTDIR\Uninstall.exe"

  ; Remove shortcuts
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  RMDir  "$SMPROGRAMS\${APP_NAME}"

  ; Remove install directory (only if empty after above deletes)
  RMDir  "$INSTDIR"

  ; Remove registry entries
  DeleteRegKey HKLM "Software\${APP_NAME}"
  DeleteRegKey HKLM "${UNINSTALL_KEY}"

SectionEnd

; ---------------------------------------------------------------------------
; Installer functions
; ---------------------------------------------------------------------------

Function .onInit
  ; Prevent running two instances of the installer simultaneously
  System::Call 'kernel32::CreateMutex(p 0, b 1, t "MowisAISetupMutex") p .r1 ?e'
  Pop $0
  StrCmp $0 0 +3
    MessageBox MB_OK|MB_ICONEXCLAMATION \
      "The ${APP_NAME} installer is already running."
    Abort

  ; Warn if a previous version is already installed
  ReadRegStr $0 HKLM "Software\${APP_NAME}" ""
  StrCmp $0 "" done
    MessageBox MB_YESNO|MB_ICONQUESTION \
      "${APP_NAME} is already installed in '$0'.$\n$\nInstall the new version over it?" \
      IDYES done
    Abort
  done:
FunctionEnd

Function un.onInit
  MessageBox MB_YESNO|MB_ICONQUESTION \
    "Are you sure you want to uninstall ${APP_NAME}?" \
    IDYES +2
  Abort
FunctionEnd
