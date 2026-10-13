; NSIS hooks for the Windows installer (tauri.conf.json bundle.windows.nsis.installerHooks).
;
; Builds up to 0.2.0 installed as "DoD Tools Studio" with identifier com.dodtools.studio.
; Tauri keys the install on productName (uninstall registry key, install folder, Start-menu
; shortcut) and the WebView2 profile on the identifier, so the renamed app installs beside
; the old one instead of upgrading it (#258). This hook finishes the move once, after the new
; files are in place: it carries the old window's saved pane choices over, then removes the
; old install. The app's own settings live in %APPDATA%\dod-studio and are untouched by both.

!define DODSTUDIO_OLD_PRODUCTNAME "DoD Tools Studio"
!define DODSTUDIO_OLD_BUNDLEID "com.dodtools.studio"

!macro NSIS_HOOK_POSTINSTALL
  ; localStorage (Blender, HD and Overviews pane choices) lives in the WebView2 profile, which
  ; follows the identifier. Copy only "Local Storage", and only into a profile that does not
  ; exist yet, so a reinstall never overwrites what the renamed app already saved.
  StrCpy $R2 "$LOCALAPPDATA\${DODSTUDIO_OLD_BUNDLEID}\EBWebView\Default\Local Storage"
  ${If} ${FileExists} "$R2\*.*"
  ${AndIfNot} ${FileExists} "$LOCALAPPDATA\${BUNDLEID}\EBWebView\*.*"
    CreateDirectory "$LOCALAPPDATA\${BUNDLEID}\EBWebView\Default\Local Storage"
    CopyFiles /SILENT "$R2\*.*" "$LOCALAPPDATA\${BUNDLEID}\EBWebView\Default\Local Storage"
  ${EndIf}

  ; Remove the old install through its own uninstaller, silently: that takes its Apps entry,
  ; Start-menu and desktop shortcuts and taskbar pin with it. Its silent run leaves the
  ; "delete app data" box unticked, so it deletes no data. `_?=` keeps it from copying itself
  ; to %TEMP% and returning before it finishes, which also leaves uninstall.exe behind.
  ReadRegStr $R1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${DODSTUDIO_OLD_PRODUCTNAME}" "InstallLocation"
  ; Tauri stores InstallLocation in quotes.
  StrCpy $R3 $R1 1
  ${If} $R3 == '"'
    StrCpy $R1 $R1 -1 1
  ${EndIf}
  ; Never run it against this install's own folder: the old uninstaller deletes files by name,
  ; and the new install ships files with the same names.
  ${If} $R1 != ""
  ${AndIf} $R1 != $INSTDIR
  ${AndIf} ${FileExists} "$R1\uninstall.exe"
    DetailPrint "Removing the old ${DODSTUDIO_OLD_PRODUCTNAME} install from $R1"
    ExecWait '"$R1\uninstall.exe" /S _?=$R1'
    Delete "$R1\uninstall.exe"
    RMDir "$R1"
  ${EndIf}
!macroend
