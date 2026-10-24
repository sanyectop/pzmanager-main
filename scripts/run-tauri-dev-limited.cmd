@echo off
setlocal
title PZManager Tauri Dev (limited token)
cd /d "%~dp0.."
npm run tauri dev
echo.
echo Tauri dev exited with code %ERRORLEVEL%.
pause
