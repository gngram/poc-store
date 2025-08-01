# Set Up NixOS Client VM

### 2.1 Set Hostname and Networking
In your configuration.nix:

```nix
{ config, pkgs, ... }:

{
  networking.hostName = "nixos-client";
  networking.domain = "example.com";
  networking.useDHCP = false;

  networking.interfaces.enp1s0.ipv4.addresses = [{
    address = "192.168.122.20";
    prefixLength = 24;
  }];
  networking.defaultGateway = "192.168.122.1";
  networking.nameservers = [ "192.168.122.10" ]; # AD DNS

  time.timeZone = "UTC";

  services.sssd = {
    enable = true;
    domains = [ "example.com" ];
    config = ''
      [sssd]
      domains = example.com
      config_file_version = 2
      services = nss, pam

      [domain/example.com]
      ad_domain = example.com
      krb5_realm = EXAMPLE.COM
      realmd_tags = manages-system joined-with-adcli
      cache_credentials = True
      id_provider = ad
      auth_provider = ad
      chpass_provider = ad
      access_provider = ad
      ldap_id_mapping = True
    '';
  };

  security.pam.enableSssd = true;
  security.sudo.extraRules = [
    { users = [ "EXAMPLE\\Administrator" ]; commands = [ "ALL" ]; }
  ];

  environment.systemPackages = with pkgs; [ sssd realmd adcli krb5 samba ];
}
```

Run nixos-rebuild switch.

### Step 3: Join NixOS to the Domain

After reboot or nixos-rebuild switch, run:

```bash
sudo realm join --user=Administrator example.com
```

Then test:

```bash
id 'EXAMPLE\\Administrator'
```

You should see UID and groups.

To login:

```bash
login: EXAMPLE\\username
password: ***********
```

