# Сборка AfterCore и раскладка в папку запуска.
#   powershell -ExecutionPolicy Bypass -File .\build.ps1          # release
#   powershell -ExecutionPolicy Bypass -File .\build.ps1 -Test    # + тесты
param([switch]$Test)

$ErrorActionPreference = 'Stop'
$src = $PSScriptRoot
$dst = Join-Path (Split-Path $src -Parent) 'AfterCore'

Push-Location $src
try {
    if ($Test) {
        cargo test --workspace
        if ($LASTEXITCODE -ne 0) { throw 'Тесты не прошли' }
    }
    cargo build --release -p aftercore
    if ($LASTEXITCODE -ne 0) { throw 'Сборка не удалась' }

    New-Item -ItemType Directory -Force $dst | Out-Null
    $exe = Join-Path $dst 'AfterCore.exe'
    $old = Join-Path $dst 'AfterCore.old.exe'
    Remove-Item $old -Force -ErrorAction SilentlyContinue
    try {
        Copy-Item (Join-Path $src 'target\release\AfterCore.exe') $exe -Force -ErrorAction Stop
    } catch {
        # AfterCore is running: a running exe can be renamed, so move it aside and copy the new one.
        Rename-Item $exe 'AfterCore.old.exe' -ErrorAction Stop
        Copy-Item (Join-Path $src 'target\release\AfterCore.exe') $exe -Force
        Write-Host 'AfterCore запущен: новая версия заработает после перезапуска, AfterCore.old.exe она удалит сама' -ForegroundColor Yellow
    }
    Write-Host "Готово: $dst\AfterCore.exe" -ForegroundColor Green
}
finally {
    Pop-Location
}
