param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$BuildProfile = "release",
    [string]$OutputDirectory = "",
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$ProjectDirectory = Split-Path $PSScriptRoot -Parent
Push-Location $ProjectDirectory
try {
    if ($Target -notmatch "-windows-") { throw "Select a Windows Rust target." }
    if (-not $SkipBuild) {
        & cargo build --locked -p repo-city --bin vibe-city --target $Target --profile $BuildProfile
        if ($LASTEXITCODE -ne 0) { throw "Windows build failed." }
    }
    $TargetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $ProjectDirectory "target" }
    $ProfileDirectory = if ($BuildProfile -eq "dev") { "debug" } else { $BuildProfile }
    $Binary = Join-Path $TargetDirectory "$Target/$ProfileDirectory/vibe-city.exe"
    if (-not (Test-Path $Binary -PathType Leaf)) { throw "Missing Windows executable: $Binary" }
    if (-not $OutputDirectory) { $OutputDirectory = Join-Path $ProjectDirectory "dist" }
    $Name = "VibeCity-$Target"
    $Bundle = Join-Path $OutputDirectory $Name
    $Licenses = Join-Path $Bundle "licenses"
    New-Item -ItemType Directory -Force $Licenses | Out-Null
    Copy-Item $Binary (Join-Path $Bundle "VibeCity.exe")
    Copy-Item LICENSE-MIT, LICENSE-APACHE, THIRD_PARTY.md, licenses/*.txt, assets/fonts/FiraMono-LICENSE, assets/fonts/NotoEmoji-LICENSE $Licenses
    @'
Launch VibeCity.exe, then choose Open local repository from the robot logo.
Enter a checkout path, or run VibeCity.exe --repo C:\path\to\checkout --live.
Install Git for Windows and allow applications to use Git from PATH.
A DirectX 12 or Vulkan-capable graphics driver is required. No gh command is needed.
System voice speech is currently available on macOS only.
'@ | Set-Content -Encoding utf8 (Join-Path $Bundle "README.txt")
    $Archive = Join-Path $OutputDirectory "$Name.zip"
    Compress-Archive -Path $Bundle -DestinationPath $Archive -Force
    Write-Output $Archive
} finally {
    Pop-Location
}
