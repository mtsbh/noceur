; Takes "Show in Noceur" off the right-click menu on uninstall, while the
; executable that knows the entries is still there. Not on an update, which
; puts the same build back.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --context-menu off'
  ${EndIf}
!macroend
