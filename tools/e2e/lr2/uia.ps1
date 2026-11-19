# UI Automation of LR2's windows (its setup dialog), from outside: what any
# screen reader sees. Windows PowerShell 5.1 (powershell.exe), for the
# UIAutomationClient assemblies.
#   powershell -File uia.ps1 -Cmd dump  -Title TITLE -Out FILE [-Shots DIR]
#       every control of the window, each tab selected in turn (a
#       screenshot of each into DIR)
#   powershell -File uia.ps1 -Cmd do    -Title TITLE -Script FILE
#       the actions of FILE, one a line (see Act below)
param(
    [Parameter(Mandatory)] [string] $Cmd,
    [Parameter(Mandatory)] [string] $Title,
    [string] $Out = '',
    [string] $Shots = '',
    [string] $Script = '',
    [int] $Wait = 30
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [Windows.Automation.AutomationElement]
$T = [Windows.Automation.TreeScope]

function Find-Window([string] $title) {
    $cond = New-Object Windows.Automation.PropertyCondition($A::NameProperty, $title)
    for ($i = 0; $i -lt $Wait * 4; $i++) {
        $w = $A::RootElement.FindFirst($T::Children, $cond)
        if ($w) { return $w }
        Start-Sleep -Milliseconds 250
    }
    throw "no window '$title'"
}

function Describe($e) {
    $c = $e.Current
    $s = "$($c.ControlType.ProgrammaticName -replace 'ControlType\.', '') '$($c.Name)' id=$($c.AutomationId) class=$($c.ClassName)"
    if (-not $c.IsEnabled) { $s += ' disabled' }
    $p = $null
    if ($e.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref] $p)) { $s += " toggle=$($p.Current.ToggleState)" }
    if ($e.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref] $p)) { $s += " value='$($p.Current.Value)'" }
    if ($e.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref] $p)) { $s += " selected=$($p.Current.IsSelected)" }
    $r = $c.BoundingRectangle
    if (-not $r.IsEmpty) { $s += " at $([int]$r.X),$([int]$r.Y) $([int]$r.Width)x$([int]$r.Height)" }
    return $s
}

function Walk($e, [int] $depth) {
    $walker = [Windows.Automation.TreeWalker]::ControlViewWalker
    $c = $walker.GetFirstChild($e)
    while ($c) {
        ('  ' * $depth) + (Describe $c)
        Walk $c ($depth + 1)
        $c = $walker.GetNextSibling($c)
    }
}

function All($w, [string] $type) {
    $ct = [Windows.Automation.ControlType]::$type
    $cond = New-Object Windows.Automation.PropertyCondition($A::ControlTypeProperty, $ct)
    return @($w.FindAll($T::Descendants, $cond))
}

# the first control of TYPE whose name matches NAME (a regex), or whose
# AutomationId is NAME given as #ID
function Control($w, [string] $type, [string] $name) {
    foreach ($e in (All $w $type)) {
        if ($name.StartsWith('#')) { if ($e.Current.AutomationId -eq $name.Substring(1)) { return $e } }
        elseif ($e.Current.Name -match $name) { return $e }
    }
    throw "no $type '$name'"
}

function Shot([string] $file) {
    if ($Shots) { & ffmpeg -hide_banner -loglevel error -y -f gdigrab -framerate 5 -i desktop -frames:v 1 (Join-Path $Shots $file) 2>&1 | Out-Host }
}

# One action a line (Act):
#   tab NAME              select the tab
#   click NAME            press the button
#   check NAME on|off     set the check box
#   radio NAME            select the radio button
#   set #ID TEXT          type TEXT into the edit box (by AutomationId)
#   combo #ID TEXT        set the combo box's text
#   shot FILE             screenshot
#   sleep MS
#   dump FILE             the control tree, appended to FILE
function Act($w, [string] $line) {
    $verb, $rest = $line.Trim() -split '\s+', 2
    switch ($verb) {
        'tab' { (Control $w 'TabItem' $rest).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select(); Start-Sleep -Milliseconds 300 }
        'click' { (Control $w 'Button' $rest).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke() }
        'check' {
            $name, $want = $rest -split '\s+(?=(on|off)$)', 2
            $p = (Control $w 'CheckBox' $name).GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
            $on = $want -match 'on'
            if (($p.Current.ToggleState -eq 'On') -ne $on) { $p.Toggle() }
        }
        'radio' { (Control $w 'RadioButton' $rest).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select() }
        'set' {
            $id, $text = $rest -split '\s+', 2
            (Control $w 'Edit' $id).GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($text)
        }
        'combo' {
            $id, $text = $rest -split '\s+', 2
            $cb = Control $w 'ComboBox' $id
            $edit = @($cb.FindAll($T::Descendants, (New-Object Windows.Automation.PropertyCondition($A::ControlTypeProperty, [Windows.Automation.ControlType]::Edit))))
            $target = if ($edit.Count) { $edit[0] } else { $cb }
            $target.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($text)
        }
        'shot' { Shot $rest }
        'sleep' { Start-Sleep -Milliseconds ([int]$rest) }
        'dump' { Walk $w 0 | Out-File -Append -Encoding utf8 $rest }
        default { if ($verb -and -not $verb.StartsWith('#')) { throw "unknown action $verb" } }
    }
    Write-Host "uia: $line"
}

$w = Find-Window $Title
switch ($Cmd) {
    'dump' {
        $lines = @("=== $(Describe $w)")
        $lines += Walk $w 0
        $tabs = All $w 'TabItem'
        $i = 0
        foreach ($tab in $tabs) {
            $tab.GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select()
            Start-Sleep -Milliseconds 500
            Shot ("setup-tab{0}.png" -f $i++)
            $lines += "=== tab '$($tab.Current.Name)'"
            $lines += Walk $w 0
        }
        if ($tabs.Count) { $tabs[0].GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select() }
        # the menu bar's menus, opened one by one (their items are a popup
        # window of their own)
        foreach ($m in (All $w 'MenuItem')) {
            $p = $null
            if (-not $m.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref] $p)) { continue }
            try { $p.Expand() } catch { continue }
            Start-Sleep -Milliseconds 500
            Shot ("setup-menu-{0}.png" -f ($m.Current.Name -replace '\W', ''))
            $lines += "=== menu '$($m.Current.Name)'"
            $lines += Walk $m 0
            $menus = $A::RootElement.FindAll($T::Children, (New-Object Windows.Automation.PropertyCondition($A::ClassNameProperty, '#32768')))
            foreach ($pop in $menus) { $lines += Walk $pop 1 }
            try { $p.Collapse() } catch { }
        }
        $lines | Out-File -Encoding utf8 $Out
    }
    'do' {
        foreach ($line in Get-Content -Encoding utf8 $Script) { if ($line.Trim()) { Act $w $line } }
    }
    default { throw "unknown command $Cmd" }
}
