import sqlite3, os
path = os.path.join(os.environ['APPDATA'], 'RustDesk', 'config', 'db_v2.sqlite3')
c = sqlite3.connect(path)
print('Tables:', [r[0] for r in c.execute("SELECT name FROM sqlite_master WHERE type='table'")])
for t in [r[0] for r in c.execute("SELECT name FROM sqlite_master WHERE type='table'")]:
    rows = c.execute(f"SELECT * FROM {t}").fetchall()
    print(f'  {t}: {len(rows)} rows')
c.close()
