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

; A game left open across an update still has the hook DLL loaded from the install folder, and
; Windows refuses to overwrite or delete a loaded DLL, only to rename it. Tauri's `File` would
; then stop the update on an Abort/Retry/Ignore box, and Ignore keeps the old DLL until the next
; release (#1143). So the DLL is moved aside first: the new one installs, the open game runs the
; old code from the renamed file, and Studio's hook build check asks to restart that game. A
; `.old` copy no game holds any more is deleted on the next install or the uninstall.
!macro DODSTUDIO_MOVE_HOOK_DLL_ASIDE DIR
  Delete "${DIR}\dodstudio_goldsrc_hooks.*.old"
  ${If} ${FileExists} "${DIR}\dodstudio_goldsrc_hooks.dll"
    ClearErrors
    Delete "${DIR}\dodstudio_goldsrc_hooks.dll"
    ${If} ${Errors}
      System::Call 'kernel32::GetTickCount()i.R8'
      Rename "${DIR}\dodstudio_goldsrc_hooks.dll" "${DIR}\dodstudio_goldsrc_hooks.$R8.old"
      DetailPrint "The hook DLL is in use by an open game: moved aside to dodstudio_goldsrc_hooks.$R8.old"
    ${EndIf}
  ${EndIf}
  ClearErrors
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; Where the resource lands today, and where builds before Tauri's layout change put it.
  !insertmacro DODSTUDIO_MOVE_HOOK_DLL_ASIDE "$INSTDIR\goldsrc-hooks"
  !insertmacro DODSTUDIO_MOVE_HOOK_DLL_ASIDE "$INSTDIR\resources\goldsrc-hooks"
!macroend

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
    ; Whether the old install had a desktop shortcut, before its uninstaller removes it.
    StrCpy $R4 0
    ${If} ${FileExists} "$DESKTOP\${DODSTUDIO_OLD_PRODUCTNAME}.lnk"
      StrCpy $R4 1
    ${EndIf}
    DetailPrint "Removing the old ${DODSTUDIO_OLD_PRODUCTNAME} install from $R1"
    ExecWait '"$R1\uninstall.exe" /S _?=$R1'
    Delete "$R1\uninstall.exe"
    ; Files older builds installed that the last old uninstaller no longer lists, by name only,
    ; so the empty folders can go; anything else keeps its folder.
    Delete "$R1\resources\goldsrc-hooks\dodstudio_goldsrc_hooks.dll"
    RMDir "$R1\resources\goldsrc-hooks"
    RMDir "$R1\resources"
    Delete "$R1\localizations\dod_tools_english.txt"
    RMDir "$R1\localizations"
    RMDir "$R1"

    ; The updater runs this installer in update mode, which never creates shortcuts: Tauri
    ; assumes the app's own are already there. Under the new name they aren't (the old
    ; uninstaller just took the old ones), so the renamed app had no Start-menu entry (#258).
    ${IfNot} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    ${EndIf}
    ${If} $R4 = 1
    ${AndIfNot} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
      CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}

  ; Tauri writes the .dodstudio association (bundle.fileAssociations, #528) but never tells
  ; Explorer, which then shows project files with no icon until it restarts.
  !insertmacro UPDATEFILEASSOC
!macroend

; Settings -> "Show DoD Studio in Open With for .dem files" (#528) adds a ProgID and an
; OpenWithProgids value under HKCU (studio/src-tauri/src/dem_open_with.rs). An uninstall takes
; them out when they open this install's exe. An update never runs the uninstaller (and the
; $UpdateMode check covers one started with /UPDATE), and a dev build's entry opens another exe,
; so both keep theirs. .dem's own default handler was never touched: nothing to restore.
!macro NSIS_HOOK_POSTUNINSTALL
  ; DLLs a game held during an install (see DODSTUDIO_MOVE_HOOK_DLL_ASIDE); one still held stays.
  Delete "$INSTDIR\goldsrc-hooks\dodstudio_goldsrc_hooks.*.old"
  Delete "$INSTDIR\resources\goldsrc-hooks\dodstudio_goldsrc_hooks.*.old"
  ${If} $UpdateMode <> 1
    ReadRegStr $R7 HKCU "Software\Classes\DoDStudio.Demo\shell\open\command" ""
    ${If} $R7 == '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"'
      DeleteRegKey HKCU "Software\Classes\DoDStudio.Demo"
      DeleteRegValue HKCU "Software\Classes\.dem\OpenWithProgids" "DoDStudio.Demo"
      !insertmacro UPDATEFILEASSOC
    ${EndIf}
  ${EndIf}
!macroend
