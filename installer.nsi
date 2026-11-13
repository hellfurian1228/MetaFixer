!include "MUI2.nsh"

Name "Meta Fixer"
OutFile "dist/MetaFixer-Setup.exe"
InstallDir "$PROGRAMFILES64\Meta Fixer"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "dist/MetaFixer.exe"
  CreateShortCut "$DESKTOP\Meta Fixer.lnk" "$INSTDIR\MetaFixer.exe"
  CreateDirectory "$SMPROGRAMS"
  CreateShortCut "$SMPROGRAMS\Meta Fixer.lnk" "$INSTDIR\MetaFixer.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MetaFixer" "DisplayName" "Meta Fixer"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MetaFixer" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MetaFixer" "DisplayIcon" "$INSTDIR\MetaFixer.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MetaFixer" "Publisher" "Meta Fixer"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\MetaFixer.exe"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$DESKTOP\Meta Fixer.lnk"
  Delete "$SMPROGRAMS\Meta Fixer.lnk"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MetaFixer"
SectionEnd
