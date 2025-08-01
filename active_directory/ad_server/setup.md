##  Step 1: Set Up AD Server (Ubuntu/Debian VM)

### 1.1 Install Required Packages

```bash
sudo apt update
sudo apt install samba smbclient krb5-user winbind dnsutils -y
```

### 1.2 Stop and Disable Old Samba Services

```bash
sudo systemctl stop smbd nmbd winbind
sudo systemctl disable smbd nmbd winbind
```

### 1.3 Provision the AD Domain

```bash
sudo samba-tool domain provision \
  --use-rfc2307 \
  --realm=EXAMPLE.COM \
  --domain=EXAMPLE \
  --adminpass='StrongAdminPass123!' \
  --server-role=dc
```

Choose internal DNS when asked.

### 1.4 Update /etc/krb5.conf
Replace with:

``` ini
[libdefaults]
 default_realm = EXAMPLE.COM
 dns_lookup_realm = false
 dns_lookup_kdc = true
```

### 1.5 Start Samba AD DC

```bash
sudo systemctl unmask samba-ad-dc
sudo systemctl enable samba-ad-dc
sudo systemctl start samba-ad-dc
```

### 1.6 Test AD Server

```bash
kinit Administrator
klist
```
