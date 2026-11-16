# Auxiliar fixo do aplicativo. Caminhos são dados de ambiente, nunca código avaliado.
# Mantém a trava do instalador mesmo depois que o ConsoloStruct encerra.
$ErrorActionPreference = 'Stop'
$guard = $null
$package = $null
$started = $false
$failed = $false
$kind = 'io'
try {
    $guard = [System.IO.File]::Open($env:CONSOLO_UPDATE_LOCK, [System.IO.FileMode]::OpenOrCreate, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::ReadWrite)
    $kind = 'busy'
    $guard.Lock(0, 1)
    $kind = 'io'
    # Read permite executar o pacote e impede outro escritor de alterá-lo durante o uso.
    $package = [System.IO.File]::Open($env:CONSOLO_UPDATE_PACKAGE, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $digest = [System.BitConverter]::ToString($algorithm.ComputeHash($package)).Replace('-', '').ToLowerInvariant()
    } finally {
        $algorithm.Dispose()
    }
    if ($package.Length -ne [long]$env:CONSOLO_UPDATE_SIZE -or $digest -ne $env:CONSOLO_UPDATE_SHA256) {
        $kind = 'integrity'
        throw 'O pacote foi alterado antes da instalação e não será executado.'
    }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $env:CONSOLO_UPDATE_PACKAGE
    # /D é o último argumento e não é colocado entre aspas, conforme o NSIS.
    $info.Arguments = '/P /R /UPDATE /D=' + $env:CONSOLO_UPDATE_DIRECTORY
    $info.UseShellExecute = $false
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    if (-not $process.Start()) { throw 'Não foi possível iniciar o instalador.' }
    $started = $true
    [System.IO.File]::WriteAllText($env:CONSOLO_UPDATE_STATUS, '{"ready":true}')
    $process.WaitForExit()
} catch {
    $failed = $true
    if (-not $started) {
        $status = @{ ready = $false; kind = $kind; error = $_.Exception.Message } | ConvertTo-Json -Compress
        [System.IO.File]::WriteAllText($env:CONSOLO_UPDATE_STATUS, $status)
    }
} finally {
    if ($package) { $package.Dispose() }
    if ($started) { Remove-Item -LiteralPath $env:CONSOLO_UPDATE_PACKAGE -ErrorAction SilentlyContinue }
    if ($guard) { $guard.Dispose() }
}
if ($failed) { exit 1 }
