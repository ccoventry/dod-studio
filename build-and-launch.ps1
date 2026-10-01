$ExeName = "desktop-studio"
$ExePath = "target\release\$ExeName.exe"

# 1. Proactively drop file locks
Stop-Process -Name $ExeName -Force -ErrorAction SilentlyContinue

# 2. Build the goldsrc-hooks DLL (32-bit, injected into hl.exe). Nothing else
# builds it: the Studio build only copies whatever is already in
# target\i686-pc-windows-msvc\release, so a stale DLL would ship silently.
cargo build --release -p goldsrc-hooks --target i686-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# 3. Build the frontend — tauri.conf.json's frontendDist ("../dist") is embedded
# into the binary at compile time, so it must exist and be current before cargo build.
Push-Location studio
npm run build
$frontendExit = $LASTEXITCODE
Pop-Location
if ($frontendExit -ne 0) { exit $frontendExit }

# 4. Build the Tauri backend
# --features tauri/custom-protocol is required for a real production build:
# without it, tauri-macros' generate_context! bakes in `dev: true` regardless
# of --release, so the binary loads the (nonexistent, once npm run build has
# finished) Vite dev server at devUrl instead of the embedded frontendDist.
cargo build --release -p $ExeName --features tauri/custom-protocol
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# 5. Sign the binary to bypass WDAC
$cert = Get-Item Cert:\CurrentUser\My\* | Where-Object { $_.Subject -match 'LocalRustDev' } | Select-Object -First 1
if ($cert -and (Test-Path $ExePath)) {
    Set-AuthenticodeSignature -Certificate $cert -FilePath $ExePath | Out-Null
}

# 6. Execute and pass arguments
& $ExePath $args
