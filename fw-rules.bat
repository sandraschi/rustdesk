@echo off
netsh advfirewall firewall add rule name="RustDesk hbbs" dir=in action=allow program="D:\Dev\repos\rustdesk-server\target\release\hbbs.exe" enable=yes protocol=any
netsh advfirewall firewall add rule name="RustDesk hbbr" dir=in action=allow program="D:\Dev\repos\rustdesk-server\target\release\hbbr.exe" enable=yes protocol=any
netsh advfirewall firewall add rule name="RustDesk Ports 21115-21119" dir=in action=allow protocol=TCP localport=21115-21119 enable=yes
netsh advfirewall firewall add rule name="RustDesk UDP 21116" dir=in action=allow protocol=UDP localport=21116 enable=yes
echo Done.
