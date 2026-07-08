import sqlite3
import os
path = os.path.join(os.environ['APPDATA'], 'RustDesk', 'config', 'db_v2.sqlite3')
c = sqlite3.connect(path)
tables = [r[0] for r in c.execute("SELECT name FROM sqlite_master WHERE type='table'")]
print("Tables:", tables)
for t in tables:
    cols = [r[1] for r in c.execute(f"PRAGMA table_info({t})")]
    print(f"\n{t}: {cols}")
    if t in ('peers', 'devices', 'sessions', 'clients'):
        rows = c.execute(f"SELECT * FROM {t} LIMIT 5").fetchall()
        for row in rows:
            print(" ", row)
c.close()
