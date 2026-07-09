import base64, os, re

toml_path = os.path.join(os.environ['APPDATA'], 'RustDesk', 'config', 'RustDesk.toml')
with open(toml_path) as f:
    content = f.read()

# Extract the key_pair as raw bytes from TOML multi-line arrays
m = re.search(r'key_pair\s*=\s*\[(.*?)\]\s*\n\S', content, re.DOTALL)
if not m:
    print("key_pair not found")
    exit(1)

raw = m.group(1)
# Extract individual arrays: each is [n,n,n,...]
arrays = re.findall(r'\[([\d,\s]+)\]', raw)
print(f"Found {len(arrays)} arrays")
if len(arrays) < 2:
    print("Not enough arrays")
    exit(1)

priv_key = bytes(int(x) for x in arrays[0].split(',') if x.strip())
pub_key = bytes(int(x) for x in arrays[1].split(',') if x.strip())

priv_b64 = base64.b64encode(priv_key).decode()
pub_b64 = base64.b64encode(pub_key).decode()

print("Client private key:", priv_b64)
print("Client public key: ", pub_b64)

data_dir = 'D:\\Dev\\repos\\rustdesk-server\\data'
with open(os.path.join(data_dir, 'id_ed25519'), 'wb') as f:
    f.write(priv_b64.encode())
with open(os.path.join(data_dir, 'id_ed25519.pub'), 'wb') as f:
    f.write(pub_b64.encode())
print("Keys written to", data_dir)
print("\nKey for iPad/other clients:", pub_b64)
