@echo off
rem Starts Rusted in this window. Type commands here (help, stop, op <name>...).
rem If the server crashes it starts again by itself; "stop" closes it for good.
title Rusted server
cd /d "%~dp0"
:start
rusted.exe
if %errorlevel%==0 goto end
echo.
echo Rusted stopped unexpectedly (code %errorlevel%). Restarting in 5 seconds, close this window to cancel.
timeout /t 5 >nul
goto start
:end
echo.
echo Rusted has stopped.
pause
