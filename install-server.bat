@echo off
sc stop RustDeskHbbs 2>nul
sc stop RustDeskHbbr 2>nul
sc delete RustDeskHbbs 2>nul
sc delete RustDeskHbbr 2>nul

set HBB_DIR=D:\Dev\repos\rustdesk-server\target\release
set HBB_DATA=D:\Dev\repos\rustdesk-server\data

schtasks /Create /SC ONSTART /TN "RustDesk hbbs" /TR "%HBB_DATA%\start_hbbs.bat" /RL HIGHEST /F
schtasks /Create /SC ONSTART /TN "RustDesk hbbr" /TR "cmd /c cd /d %HBB_DATA% & %HBB_DIR%\hbbr.exe" /RL HIGHEST /F

echo Starting tasks...
schtasks /Run /TN "RustDesk hbbs"
schtasks /Run /TN "RustDesk hbbr"
timeout /t 3 /nobreak >nul

echo.
sc query RustDeskHbbs 2>nul | findstr STATE
sc query RustDeskHbbr 2>nul | findstr STATE
echo Done.
