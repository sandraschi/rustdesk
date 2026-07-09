@echo off
sc stop RustDeskHbbs 2>nul
sc stop RustDeskHbbr 2>nul
sc delete RustDeskHbbs 2>nul
sc delete RustDeskHbbr 2>nul
echo Uninstalled.
