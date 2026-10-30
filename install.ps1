$ErrorActionPreference = "Stop"

$repo = "tamadeu/sun"
$installDir = if ($env:SUN_INSTALL_DIR) { $env:SUN_INSTALL_DIR } else { Join-Path $HOME ".sun\bin" }
$asset = "sun-windows-x86_64.zip"
$url = "https://github.com/$repo/releases/latest/download/$asset"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("sun-install-" + [guid]::NewGuid().ToString("N"))

try {
    New-Item -ItemType Directory -Path $tempDir | Out-Null
    $archive = Join-Path $tempDir "sun.zip"
    Write-Host "Downloading Sun..."
    Invoke-WebRequest -Uri $url -OutFile $archive
    Expand-Archive -Path $archive -DestinationPath $tempDir
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Copy-Item (Join-Path $tempDir "sun.exe") (Join-Path $installDir "sun.exe") -Force
    Write-Host "Sun installed to $installDir\sun.exe"
    if (($env:Path -split ";") -notcontains $installDir) {
        Write-Host "Add $installDir to your user PATH, then open a new terminal."
    }
    Write-Host "Run: sun --help"
}
catch {
    Write-Error "Could not install Sun. A published release may not exist yet. Check https://github.com/$repo/releases"
    throw
}
finally {
    if (Test-Path $tempDir) { Remove-Item $tempDir -Recurse -Force }
}
