# Sammy

The application runs once then exits.

## Usage in cron

```crontab
5 * * * * /usr/bin/flock -n /tmp/sammy.lockfile bin/sammy | grep -v INFO
```
