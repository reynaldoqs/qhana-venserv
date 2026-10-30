@echo off
rem Lanzador para Windows. Copia o enlaza como saldo.cmd / transferir.cmd si quieres atajos.
set "NAME=%~n0"
if /I "%NAME%"=="saldo" (py -3 "%~dp0tigo.py" saldo %* & exit /b %errorlevel%)
if /I "%NAME%"=="transferir" (py -3 "%~dp0tigo.py" transferir %* & exit /b %errorlevel%)
py -3 "%~dp0tigo.py" %*
