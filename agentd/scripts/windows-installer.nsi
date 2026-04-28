; MowisAI Windows Installer Script (NSIS)
; Packages mowis-gui, agentd, Alpine WSL2 tarball, and QEMU

!include "MUI2.nsh"

; General
Name "MowisAI"
OutFile "..\..\target\windows\MowisAI-Setup.exe"
InstallDir "$LOCALAPPDATA\MowisAI"
InstallDirRegKey HKCU "Software\MowisAI" "InstallDir"
RequestExecutionLevel user

; Interface Settings
!define MUI_ABORTWARNING
!define MUI_ICON "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"

; Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; Languages
!insertmacro MUI_LANGUAGE "English"

; Installer Sections
Section "MowisAI" SecMain
    SetOutPath "$INSTDIR"
    
    ; Main executable
    File "..\..\target\release\mowisai.exe"
    
    ; Alpine WSL2 tarball
    File "..\..\target\images\alpine-wsl2-x86_64.tar.gz"
    
    ; QEMU binary (fallback)
    File /nonfatal "..\..\target\qemu\qemu-system-x86_64.exe"
    
    ; Checksums
    File /nonfatal "..\..\target\images\checksums.txt"
    
    ; Create uninstaller
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    
    ; Registry keys
    WriteRegStr HKCU "Software\MowisAI" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MowisAI" \
                     "DisplayName" "MowisAI"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MowisAI" \
                     "UninstallString" "$INSTDIR\Uninstall.exe"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MowisAI" \
                     "DisplayIcon" "$INSTDIR\mowisai.exe"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MowisAI" \
                     "Publisher" "MowisAI"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MowisAI" \
                     "DisplayVersion" "0.1.0"
    
    ; Start Menu shortcuts
    CreateDirectory "$SMPROGRAMS\MowisAI"
    CreateShortcut "$SMPROGRAMS\MowisAI\MowisAI.lnk" "$INSTDIR\mowisai.exe"
    CreateShortcut "$SMPROGRAMS\MowisAI\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
    
    ; Desktop shortcut
    CreateShortcut "$DESKTOP\MowisAI.lnk" "$INSTDIR\mowisai.exe"
SectionEnd

; Uninstaller Section
Section "Uninstall"
    ; Remove files
    Delete "$INSTDIR\mowisai.exe"
    Delete "$INSTDIR\alpine-wsl2-x86_64.tar.gz"
    Delete "$INSTDIR\qemu-system-x86_64.exe"
    Delete "$INSTDIR\checksums.txt"
    Delete "$INSTDIR\Uninstall.exe"
    
    ; Remove directories
    RMDir "$INSTDIR"
    
    ; Remove shortcuts
    Delete "$SMPROGRAMS\MowisAI\MowisAI.lnk"
    Delete "$SMPROGRAMS\MowisAI\Uninstall.lnk"
    RMDir "$SMPROGRAMS\MowisAI"
    Delete "$DESKTOP\MowisAI.lnk"
    
    ; Remove registry keys
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MowisAI"
    DeleteRegKey HKCU "Software\MowisAI"
    
    ; Unregister WSL2 distribution (if exists)
    ExecWait 'wsl --unregister MowisAI'
SectionEnd
