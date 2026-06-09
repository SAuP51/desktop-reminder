param(
    [switch]$RequireRust,
    [string]$CargoPath = $env:CARGO
)

$ErrorActionPreference = "Stop"

function Resolve-Cargo {
    if ($CargoPath) {
        if (Test-Path -LiteralPath $CargoPath) {
            return (Resolve-Path -LiteralPath $CargoPath).Path
        }

        throw "CargoPath was provided but does not exist: $CargoPath"
    }

    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargo) {
        return $cargo.Source
    }

    return $null
}

function Test-WindowsMsvcBuildTools {
    if ($env:OS -ne "Windows_NT") {
        return $true
    }

    $missing = @()
    if (-not (Get-Command link.exe -ErrorAction SilentlyContinue)) {
        $missing += "link.exe"
    }

    $libDirs = @()
    if ($env:LIB) {
        $libDirs = $env:LIB -split ';' | Where-Object { $_ }
    }

    $hasKernel32Lib = $false
    foreach ($dir in $libDirs) {
        if (Test-Path -LiteralPath (Join-Path $dir "kernel32.lib")) {
            $hasKernel32Lib = $true
            break
        }
    }

    if (-not $hasKernel32Lib) {
        $missing += "kernel32.lib in LIB"
    }

    if ($missing.Count -eq 0) {
        return $true
    }

    $message = "Skipping Rust validation because Windows MSVC build tools are not ready. Missing: $($missing -join ', '). Install Visual Studio Build Tools with Desktop development with C++ and a Windows SDK, then run this script from Developer PowerShell or ensure PATH/LIB are configured."
    if ($RequireRust) {
        throw $message
    }

    Write-Warning $message
    return $false
}

$cargo = Resolve-Cargo
if ($cargo) {
    if (Test-WindowsMsvcBuildTools) {
        & $cargo fmt --all -- --check
        & $cargo test --workspace
        & $cargo clippy --workspace --all-targets -- -D warnings
    }
} elseif ($RequireRust) {
    throw "Rust/Cargo is required but was not found in PATH."
} else {
    Write-Warning "Skipping Rust validation because cargo was not found in PATH. Re-run with -RequireRust on a Rust-enabled machine before packaging."
}

if (Test-Path -LiteralPath "apps/reminder-ui/node_modules") {
    Push-Location "apps/reminder-ui"
    try {
        npm.cmd run build
    } finally {
        Pop-Location
    }
} else {
    Write-Host "Skipping reminder-ui frontend build; run npm install in apps/reminder-ui first."
}
