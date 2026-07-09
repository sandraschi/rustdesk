@echo off
sc stop RustDeskHbbs 2>nul
sc stop RustDeskHbbr 2>nul
sc delete RustDeskHbbs 2>nul
sc delete RustDeskHbbr 2>nul

schtasks /Create /SC ONSTART /TN "RustDesk hbbs" /TR "D:\Dev\repos\rustdesk-server\target\release\hbbs.exe" /RL HIGHEST /F
schtasks /Create /SC ONSTART /TN "RustDesk hbbr" /TR "D:\Dev\repos\rustdesk-server\target\release\hbbr.exe" /RL HIGHEST /F

echo Starting tasks...
schtasks /Run /TN "RustDesk hbbs"
schtasks /Run /TN "RustDesk hbbr"
timeout /t 3 /nobreak >nul
sc query RustDeskHbbs 2>nul | findstr STATE
sc query RustDeskHbbr 2>nul | findstr STATE
echo.
echo Done.
