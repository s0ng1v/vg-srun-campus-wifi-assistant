; Installer hooks for the NSIS bundle.
;
; The daemon, notifier and setup helper must live next to the main executable:
; the GUI resolves them as siblings. Depending on the bundler version the
; resource map may land under $INSTDIR\resources, so copy them up if needed.

!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$INSTDIR\resources\vg-campus-daemon.exe" 0 vg_helpers_done
  CopyFiles /SILENT "$INSTDIR\resources\vg-campus-daemon.exe" "$INSTDIR"
  CopyFiles /SILENT "$INSTDIR\resources\vg-campus-notifier.exe" "$INSTDIR"
  CopyFiles /SILENT "$INSTDIR\resources\vg-campus-setup.exe" "$INSTDIR"
  Delete "$INSTDIR\resources\vg-campus-daemon.exe"
  Delete "$INSTDIR\resources\vg-campus-notifier.exe"
  Delete "$INSTDIR\resources\vg-campus-setup.exe"
  RMDir "$INSTDIR\resources"
  vg_helpers_done:
!macroend

; Stop the running guard, then remove the SYSTEM scheduled tasks and URL
; protocol before deleting files. The setup helper keeps user data under
; %ProgramData% untouched.
!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'schtasks /End /TN VGCampusWifiAssistantDaemon'
  nsExec::ExecToLog 'schtasks /End /TN VGCampusWifiAssistantDaemonOnDemand'
  IfFileExists "$INSTDIR\vg-campus-setup.exe" 0 vg_uninstall_done
  ExecWait '"$INSTDIR\vg-campus-setup.exe" uninstall'
  vg_uninstall_done:
!macroend