@echo off
REM Ralph Wiggum for Claude Code - Windows batch wrapper
REM Usage: ralph [task-directory] [-i iterations] [-y]
REM
REM This is a convenience wrapper that calls ralph.ps1

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0ralph.ps1" %*
