import asyncio

from sshpro.localexec import run_local


def test_run_local_output_exit_code_and_cwd(tmp_path):
    res = asyncio.run(run_local("Write-Output 'héllo'; (Get-Location).Path", cwd=str(tmp_path)))
    assert res["exit"] == 0 and res["error"] is None
    assert res["stdout"].splitlines() == ["héllo", str(tmp_path)]
    assert asyncio.run(run_local("cmd /c exit 7"))["exit"] == 7
    assert asyncio.run(run_local(r"Get-Item C:\does-not-exist"))["exit"] == 1


def test_run_local_timeout_and_missing_folder(tmp_path):
    res = asyncio.run(run_local("Start-Sleep 30", timeout=2))
    assert res["exit"] is None and "Timed out" in res["error"] and res["duration"] < 15
    assert "Folder not found" in asyncio.run(run_local("dir", cwd=str(tmp_path / "nope")))["error"]
