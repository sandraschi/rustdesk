@echo off
net session >nul 2>&1
if %errorLevel% neq 0 ( echo Please run as Administrator & pause & exit /b 1 )

set NSSM="C:\Program Files\Jellyfin\Server\nssm.exe"
set HBB_DIR=D:\Dev\repos\rustdesk-server\target\release
set HBB_DATA=D:\Dev\repos\rustdesk-server\data

%NSSM% stop RustDesk-HBBS 2>nul
%NSSM% remove RustDesk-HBBS confirm 2>nul
%NSSM% stop RustDesk-HBBR 2>nul
%NSSM% remove RustDesk-HBBR confirm 2>nul

%NSSM% install RustDesk-HBBS "%HBB_DATA%\run-hbbs-service.bat"
%NSSM% set RustDesk-HBBS AppDirectory "%HBB_DATA%"
%NSSM% set RustDesk-HBBS AppStdout "%HBB_DATA%\logs\hbbs-stdout.log"
%NSSM% set RustDesk-HBBS AppStderr "%HBB_DATA%\logs\hbbs-stderr.log"
%NSSM% set RustDesk-HBBS Start SERVICE_AUTO_START
%NSSM% set RustDesk-HBBS AppRotateFiles 1
%NSSM% set RustDesk-HBBS AppRotateSeconds 86400
%NSSM% set RustDesk-HBBS AppRotateBytes 10485760

%NSSM% install RustDesk-HBBR "%HBB_DIR%\hbbr.exe"
%NSSM% set RustDesk-HBBR AppDirectory "%HBB_DATA%"
%NSSM% set RustDesk-HBBR AppStdout "%HBB_DATA%\logs\hbbr-stdout.log"
%NSSM% set RustDesk-HBBR AppStderr "%HBB_DATA%\logs\hbbr-stderr.log"
%NSSM% set RustDesk-HBBR Start SERVICE_AUTO_START
%NSSM% set RustDesk-HBBR AppRotateFiles 1
%NSSM% set RustDesk-HBBR AppRotateSeconds 86400
%NSSM% set RustDesk-HBBR AppRotateBytes 10485760

%NSSM% start RustDesk-HBBS
%NSSM% start RustDesk-HBBR
echo Done.
