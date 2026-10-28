!macro NSIS_HOOK_PREINSTALL
  ; Close an older Success Timer process before replacing its executable.
  nsExec::ExecToLog 'taskkill /F /IM mokpyo-jipjung.exe /T'
!macroend
