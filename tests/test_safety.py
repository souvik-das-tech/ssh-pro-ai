import pytest

from sshpro.safety import is_local_read_only, is_read_only, prepare

READ = [
    "df -h",
    "df -h / | tail -1",
    "free -m && uptime",
    "command -v php >/dev/null 2>&1 && php -v | head -1 || echo NOT_INSTALLED",
    "node -v 2>/dev/null || echo NOT_INSTALLED",
    "systemctl status nginx --no-pager | head -5",
    "docker ps --format '{{.Names}}'",
    "apt list --upgradable 2>/dev/null | wc -l",
    "cat /etc/os-release | grep PRETTY",
    "for d in /var/www/*; do du -sh $d; done",
    "sudo -n du -sh /var/log",
    "echo $(nproc) cores",
    "awk '{print $1 > 2}' /proc/loadavg",
    "if command -v nvm >/dev/null; then nvm ls; fi",
    "ls -la /etc/nginx/sites-enabled",
    "pm2 jlist",
    "dpkg -l | grep -i nodejs",
]
CHANGE = [
    "apt-get install -y nodejs",
    "sudo apt upgrade -y",
    "rm -rf /tmp/x",
    "echo hi > /etc/motd",
    "systemctl restart nginx",
    "curl -fsSL https://deb.nodesource.com/setup_22.x | sudo bash -",
    "sed -i 's/a/b/' /etc/hosts",
    "find /var/log -name '*.gz' -delete",
    "nvm install 22",
    "npm install -g pm2",
    "docker rm -f web",
    "reboot",
    "df -h; rm -rf /",
    "echo $(rm -rf /tmp/a)",
    "cat <<EOF > /tmp/x\nhi\nEOF",
    "tee /etc/foo",
    "crontab -r",
    "node -e 'require(\"fs\").rmSync(\"/x\")'",
    "awk 'BEGIN{system(\"rm -rf /\")}'",
    "xargs rm",
]


@pytest.mark.parametrize("cmd", READ)
def test_read_only(cmd):
    assert is_read_only(cmd), cmd


@pytest.mark.parametrize("cmd", CHANGE)
def test_change(cmd):
    assert not is_read_only(cmd), cmd


def test_prepare_sudo_password_goes_to_stdin_not_command():
    line, stdin = prepare("sudo apt update && sudo apt upgrade -y", "deploy", "s3cret")
    assert "s3cret" not in line
    assert stdin == "s3cret\ns3cret\n"
    assert line.startswith("bash -lc ")
    assert line.count("sudo -S -p") == 2


def test_prepare_root_drops_sudo_and_no_password_uses_n():
    line, stdin = prepare("sudo whoami", "root", None)
    assert "sudo" not in line and stdin is None
    line, _ = prepare("sudo whoami", "deploy", None)
    assert "sudo -n" in line


LOCAL_READ = [
    r'Get-ChildItem "$env:USERPROFILE\Downloads" -Filter "*.sql.gz" | Select-Object FullName, Length',
    r"Get-ChildItem D:\proj -Recurse | Where-Object { $_.Length -gt 1GB } | Sort-Object Length",
    r"(Get-Item C:\x.zip).Length",
    r"Test-Path D:\proj\package.json; git status --short",
    "node -v; npm -v",
    r"Get-FileHash C:\x.sql.gz -Algorithm SHA256",
]
LOCAL_CHANGE = [
    "npm run build",
    r"Remove-Item C:\x -Recurse",
    r"Get-ChildItem C:\tmp | Remove-Item",
    "Get-Process node | % Kill",
    "Get-Item x | % { $_.Delete() }",
    "Get-ChildItem > out.txt",
    r"& C:\tools\x.exe",
    "$x = 1",
    r'Write-Output "$(Remove-Item C:\x)"',
    r"[IO.File]::Delete('C:\x')",
    "tar.exe -czf app.tgz -C dist .",
    "Get-Content x | Set-Content y",
]


@pytest.mark.parametrize("command", LOCAL_READ)
def test_local_read_only(command):
    assert is_local_read_only(command)


@pytest.mark.parametrize("command", LOCAL_CHANGE)
def test_local_change(command):
    assert not is_local_read_only(command)
