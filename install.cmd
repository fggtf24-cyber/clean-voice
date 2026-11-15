@echo off
rem Copies the built plugin into the system plugin folders (asks for admin rights).
rem Close Premiere / Resolve / Ableton / OBS first, otherwise the old file stays locked.
net session >nul 2>&1 || (powershell -NoProfile -Command "Start-Process -Verb RunAs -FilePath '%~f0'" & exit /b)
set "SRC=%~dp0target\bundled"
if not exist "C:\Program Files\Common Files\CLAP" mkdir "C:\Program Files\Common Files\CLAP"
xcopy /E /I /Y "%SRC%\Clean Voice.vst3" "C:\Program Files\Common Files\VST3\Clean Voice.vst3" || goto fail
copy /Y "%SRC%\Clean Voice.clap" "C:\Program Files\Common Files\CLAP\" || goto fail
echo.
echo Installed. You can close this window.
pause
exit /b 0
:fail
echo.
echo FAILED - close all DAWs/editors using the plugin and run again.
pause
exit /b 1
