# Sammy

The application runs once then exits.

## Requirements

```shell
# apt-install libssl-dev
```

## Usage in cron

```crontab
5 * * * * /usr/bin/flock -n /tmp/sammy.lockfile bin/sammy | grep -v INFO
```

Or just use it as a `systemd` service instead.
