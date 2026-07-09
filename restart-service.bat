@echo off
sc stop RustDesk
timeout /t 3 /nobreak >nul
sc start RustDesk
echo Done.
