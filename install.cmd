@echo off
REM Ralph Installation Script - Windows batch wrapper
REM Usage: install [-f|--force]
REM
REM This is a convenience wrapper that calls install.ps1

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
