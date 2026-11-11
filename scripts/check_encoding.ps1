<#
  编码规范检查脚本（报告式，防回潮；与 check_standards.ps1 同风格）

  背景（根因，勿绕过）：Windows PowerShell 5.1 解析 .ps1 只认 BOM——有 BOM 按 UTF-8，
  无 BOM 一律按系统 ANSI（中文机 GBK/CP936）解码；chcp/$OutputEncoding 救不了，因为
  解析发生在执行前。实测（2026-11）：无 BOM 含中文的脚本 0 语法错误，但字符串字面量
  静默乱码（myriad scripts\dev.ps1 8 处损坏）。
  同族问题：PS5.1 的读写 cmdlet 默认 ANSI/UTF-16，pwsh 7 默认 UTF-8；且 -Encoding UTF8
  在 5.1 写 BOM、在 7 写无 BOM——不显式指定编码 = 跨版本行为不确定。

  检查项：
    ENC1（阻断） *.ps1/*.psm1/*.psd1 含非 ASCII 却无 UTF-8 BOM
                ├─ 正文是合法 UTF-8 → 直接补 BOM 即可
                └─ 正文非 UTF-8     → 文件本身是 ANSI/GBK，需转码为 UTF-8 with BOM
                另报：带 BOM 但正文非合法 UTF-8（损坏/混编码）
                命中 → 退出码 1（可接 CI/钩子）
    ENC2（告警） Get-Content/Set-Content/Add-Content/Out-File/Export-Csv/Select-String/
                Tee-Object 出现在非注释行且未显式 -Encoding（只打印，不计退出码；
                读纯 ASCII 文件属可接受误报）

  用法：
    powershell -File deploy\check_encoding.ps1                 # NewCosmosCode 全量报告
    powershell -File scripts\check_encoding.ps1                # myriad 全量报告
    powershell -File deploy\check_encoding.ps1 -Category BOM   # 只查 BOM
    powershell -File deploy\check_encoding.ps1 -Root D:\some\dir
  -Root 默认 = 脚本所在目录的上一级（deploy\ / scripts\ → 仓库根）。
  本脚本自身按 UTF-8 with BOM 保存，避免成为自己报的违规。
#>
[CmdletBinding()]
param(
    [string]$Root,
    [ValidateSet('ALL','BOM','ENC')]
    [string]$Category = 'ALL'
)

if (-not $Root) { $Root = Split-Path $PSScriptRoot -Parent }
$Root = [System.IO.Path]::GetFullPath($Root).TrimEnd('\')
if (-not (Test-Path -LiteralPath $Root)) { Write-Error "Root 不存在: $Root"; exit 2 }

$excludeRe = '\\(node_modules|\.git|bin|obj|backup|publish|dist|\.opencode)\\'
$files = @(Get-ChildItem -Path $Root -Recurse -File -Include *.ps1,*.psm1,*.psd1 -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -notmatch $excludeRe })

$bomIssues = [System.Collections.Generic.List[string]]::new()
$encIssues = [System.Collections.Generic.List[string]]::new()

$utf8Strict = [System.Text.UTF8Encoding]::new($false, $true)   # 非法字节序列抛异常
function Rel([string]$p) { $p.Substring($script:Root.Length).TrimStart('\') }
function Test-Utf8([byte[]]$bytes, [int]$offset, [int]$count) {
    try { [void]$script:utf8Strict.GetString($bytes, $offset, $count); return $true }
    catch { return $false }
}

foreach ($f in $files) {
    $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
    $hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
    $nonAscii = $false
    foreach ($b in $bytes) { if ($b -gt 127) { $nonAscii = $true; break } }

    # ---- ENC1：BOM 违规（阻断级）----
    if (-not $hasBom -and $nonAscii) {
        if (Test-Utf8 $bytes 0 $bytes.Length) {
            $bomIssues.Add("[BOM] $(Rel $f.FullName)  无BOM且含非ASCII，正文是合法UTF-8 -> 按 UTF-8 with BOM 另存（PS5.1 现正按 ANSI 解析本文件）")
        } else {
            $bomIssues.Add("[BOM] $(Rel $f.FullName)  含非ASCII且字节非UTF-8（疑似GBK/ANSI编码）-> 转码为 UTF-8 with BOM")
        }
    } elseif ($hasBom -and $bytes.Length -gt 3) {
        if (-not (Test-Utf8 $bytes 3 ($bytes.Length - 3))) {
            $bomIssues.Add("[BOM] $(Rel $f.FullName)  带BOM但正文非合法UTF-8（损坏/混编码）-> 检查并重新保存")
        }
    }

    # ---- ENC2：读写 cmdlet 缺 -Encoding（告警级）----
    if ($Category -in 'ALL','ENC') {
        if ($hasBom) {
            $text = [System.Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
        } elseif (Test-Utf8 $bytes 0 $bytes.Length) {
            $text = [System.Text.Encoding]::UTF8.GetString($bytes)
        } else {
            $text = [System.Text.Encoding]::Default.GetString($bytes)   # ANSI 文件按系统码页读
        }
        $cmdletRe = [regex]'(?<![\w.\-])(Get-Content|Set-Content|Add-Content|Out-File|Export-Csv|Select-String|Tee-Object)(?![\w\-])'
        $inBlock = $false
        $lines = $text -split "`r?`n"
        for ($i = 0; $i -lt $lines.Count; $i++) {
            $t = $lines[$i]
            if ($inBlock) { if ($t -match '#>') { $inBlock = $false }; continue }
            $trimmed = $t.TrimStart()
            if ($trimmed.StartsWith('#')) { continue }
            if ($trimmed.StartsWith('<#')) { if ($t -notmatch '#>') { $inBlock = $true }; continue }
            if ($t -match '\$cmdletRe\s*=') { continue }   # 跳过本脚本的模式定义行（避免自报）
            $m = $cmdletRe.Match($t)
            if ($m.Success -and $t -notmatch '-Encoding\b' -and $t -notmatch '\bEncoding\s*=') {
                $encIssues.Add("[ENC] $(Rel $f.FullName):$($i + 1)  $($m.Groups[1].Value) 未显式指定 -Encoding")
            }
        }
    }
}

if ($Category -in 'ALL','BOM') { $bomIssues | ForEach-Object { Write-Host $_ -ForegroundColor Red } }
if ($Category -in 'ALL','ENC') { $encIssues | ForEach-Object { Write-Host $_ -ForegroundColor Yellow } }

Write-Host ""
Write-Host ("编码检查：扫描 {0} 个脚本，ENC1 BOM 违规 {1}，ENC2 编码告警 {2}" -f $files.Count, $bomIssues.Count, $encIssues.Count)
if ($Category -in 'ALL','BOM' -and $bomIssues.Count -gt 0) {
    Write-Host "ENC1 违规 = 0 才算通过（修法见上方各行）。" -ForegroundColor Red
    exit 1
}
exit 0
