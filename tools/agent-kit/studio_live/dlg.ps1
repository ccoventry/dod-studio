param(
    [Parameter(Mandatory = $true)][string]$Title,
    [Parameter(Mandatory = $true)][string]$Path,
    [int]$EditId = 1148,
    [int]$TimeoutSec = 20
)
# Answers a native file dialog Studio opened: puts $Path in its file-name box and presses OK,
# by window messages (WM_SETTEXT, BM_CLICK), so no focus, mouse or keyboard is involved and
# no other window can be hit. Click the button that opens the dialog first (cdp.cjs).
#
# The file-name box differs by dialog:
#   1152  folder picker (Add Folder, Link Folder): an Edit straight under the dialog
#   1148  Open (Add Demo Files, Load Project): the Edit inside ComboBoxEx32 1148
#   1001  Save (Save Project): the Edit inside a ComboBox
# The first Edit inside a ComboBox is the address bar: text there plus OK only opens that folder.
# Match the dialog by its exact title: an unanswered one from an earlier click would take the path.
#
#   powershell -ExecutionPolicy Bypass -File dlg.ps1 -Title "Select Demo Folder" -Path "D:\tests\demos" -EditId 1152
# Prints "ok", "no edit/ok" or "dialog '<title>' not found".
#
# The window walk is compiled C#: a PowerShell scriptblock passed to EnumWindows hung.
Add-Type -TypeDefinition @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class KitDialog {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr h);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, string l);
  [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  const uint WM_SETTEXT = 0x000C, BM_CLICK = 0x00F5;
  static string Cls(IntPtr h){var s=new StringBuilder(256);GetClassName(h,s,256);return s.ToString();}
  static string Txt(IntPtr h){var s=new StringBuilder(256);GetWindowText(h,s,256);return s.ToString();}
  public static string Answer(string title, string path, int editId) {
    IntPtr dlg = IntPtr.Zero;
    EnumWindows((h,l)=>{ if(IsWindowVisible(h) && Cls(h)=="#32770" && Txt(h)==title){dlg=h;return false;} return true;}, IntPtr.Zero);
    if (dlg==IntPtr.Zero) return null;
    IntPtr edit = IntPtr.Zero, ok = IntPtr.Zero;
    EnumChildWindows(dlg,(h,l)=>{
      string c=Cls(h); int id=GetDlgCtrlID(h);
      if(c=="Edit" && id==editId) edit=h;
      if(c=="Button" && id==1) ok=h;
      return true;}, IntPtr.Zero);
    if (edit==IntPtr.Zero) {
      EnumChildWindows(dlg,(h,l)=>{
        if((Cls(h)=="ComboBoxEx32" || Cls(h)=="ComboBox") && GetDlgCtrlID(h)==editId){
          EnumChildWindows(h,(k,m)=>{ if(Cls(k)=="Edit"){edit=k;return false;} return true;}, IntPtr.Zero);
          return false;}
        return true;}, IntPtr.Zero);
    }
    if (edit==IntPtr.Zero || ok==IntPtr.Zero) return "no edit/ok";
    SendMessage(edit, WM_SETTEXT, IntPtr.Zero, path);
    SendMessage(ok, BM_CLICK, IntPtr.Zero, IntPtr.Zero);
    return "ok";
  }
}
"@
$deadline = (Get-Date).AddSeconds($TimeoutSec)
while ((Get-Date) -lt $deadline) {
    $result = [KitDialog]::Answer($Title, $Path, $EditId)
    if ($result) { $result; exit }
    Start-Sleep -Milliseconds 300
}
"dialog '$Title' not found"
