Unicode true
ManifestDPIAware true
SetCompressor /SOLID zlib
RequestExecutionLevel user
Name "${NAME}"
OutFile "${OUTFILE}"
BrandingText "${NAME} ${VERSION}"
VIProductVersion "${VERSION}"
VIAddVersionKey "ProductName" "${NAME}"
VIAddVersionKey "CompanyName" "Capy Atelier"
VIAddVersionKey "FileDescription" "${NAME} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Capy Atelier"

!include "MUI2.nsh"
!define MUI_ICON "${PAYLOAD}\CapyCanvas.ico"
!define MUI_UNICON "${PAYLOAD}\CapyCanvas.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\CapyCanvas.exe"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

!define UNINSTALL "Software\Microsoft\Windows\CurrentVersion\Uninstall\${KEY}"

!macro ReleaseApp
    retry:
    ClearErrors
    IfFileExists "$INSTDIR\CapyCanvas.exe" 0 released
    Delete "$INSTDIR\CapyCanvas.exe"
    IfErrors 0 released
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Close ${NAME}, then select Retry." /SD IDCANCEL IDRETRY retry
    Abort
    released:
!macroend

Function .onInit
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${NAME}"
FunctionEnd

Function un.onInit
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${NAME}"
FunctionEnd

Section
    !insertmacro ReleaseApp
    RMDir /r "$INSTDIR"
    SetOutPath "$INSTDIR"
    File /r "${PAYLOAD}\*"
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    CreateShortcut "$SMPROGRAMS\${NAME}.lnk" "$INSTDIR\CapyCanvas.exe" "" "$INSTDIR\CapyCanvas.ico"
    WriteRegStr HKCU "Software\Classes\.capy" "" "${PROGID}"
    WriteRegStr HKCU "Software\Classes\.capy\OpenWithProgids" "${PROGID}" ""
    WriteRegStr HKCU "Software\Classes\${PROGID}" "" "Capy Canvas drawing"
    WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\CapyCanvas.ico"
    WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\CapyCanvas.exe" "%1"'
    WriteRegStr HKCU "${UNINSTALL}" "DisplayName" "${NAME}"
    WriteRegStr HKCU "${UNINSTALL}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINSTALL}" "Publisher" "Capy Atelier"
    WriteRegStr HKCU "${UNINSTALL}" "DisplayIcon" "$INSTDIR\CapyCanvas.ico"
    WriteRegStr HKCU "${UNINSTALL}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKCU "${UNINSTALL}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
    WriteRegDWORD HKCU "${UNINSTALL}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTALL}" "NoRepair" 1
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

Section "Uninstall"
    !insertmacro ReleaseApp
    Delete "$SMPROGRAMS\${NAME}.lnk"
    ReadRegStr $0 HKCU "Software\Classes\.capy" ""
    StrCmp $0 "${PROGID}" 0 +2
    DeleteRegValue HKCU "Software\Classes\.capy" ""
    DeleteRegValue HKCU "Software\Classes\.capy\OpenWithProgids" "${PROGID}"
    DeleteRegKey /ifempty HKCU "Software\Classes\.capy\OpenWithProgids"
    DeleteRegKey /ifempty HKCU "Software\Classes\.capy"
    DeleteRegKey HKCU "Software\Classes\${PROGID}"
    DeleteRegKey HKCU "${UNINSTALL}"
    RMDir /r "$INSTDIR"
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd
