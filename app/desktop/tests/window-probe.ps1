param([int]$TargetProcessId,[double]$Width=0,[double]$Height=0,[string]$OutputPath,[string]$WindowTitle='Codex · 卡点调度台',[ValidateSet('none','minimize','restore')][string]$Display='none')
$ErrorActionPreference='Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class FloatboardWindowProbe {
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [StructLayout(LayoutKind.Sequential)] public struct RECT{public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct POINT{public int x,y;}
 [StructLayout(LayoutKind.Sequential)] public struct MINMAX{public POINT reserved,maxSize,maxPosition,minTrack,maxTrack;}
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr p);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder text,int length);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT rect);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT rect);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h,int index);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int width,int height,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
 [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 public static IntPtr Main(uint target,string match){IntPtr result=IntPtr.Zero;EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);var title=new StringBuilder(300);GetWindowText(h,title,300);if(pid==target&&title.ToString()==match){result=h;return false;}return true;},IntPtr.Zero);return result;}
 public static MINMAX Limits(IntPtr h){var p=Marshal.AllocHGlobal(Marshal.SizeOf<MINMAX>());try{Marshal.StructureToPtr(new MINMAX(),p,false);SendMessage(h,0x24,IntPtr.Zero,p);return Marshal.PtrToStructure<MINMAX>(p);}finally{Marshal.FreeHGlobal(p);}}
}
'@
[FloatboardWindowProbe]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$taskHandle=[FloatboardWindowProbe]::Main($TargetProcessId,$WindowTitle)
if($taskHandle -eq [IntPtr]::Zero){throw '指定验收进程的主窗不存在'}
if($Display -eq 'minimize'){[FloatboardWindowProbe]::ShowWindow($taskHandle,6)|Out-Null}
if($Display -eq 'restore'){[FloatboardWindowProbe]::ShowWindow($taskHandle,9)|Out-Null}
$taskClient=New-Object FloatboardWindowProbe+RECT
$taskOuter=New-Object FloatboardWindowProbe+RECT
[FloatboardWindowProbe]::GetClientRect($taskHandle,[ref]$taskClient)|Out-Null
[FloatboardWindowProbe]::GetWindowRect($taskHandle,[ref]$taskOuter)|Out-Null
$taskScale=[FloatboardWindowProbe]::GetDpiForWindow($taskHandle)/96.0
$taskFrameWidth=($taskOuter.right-$taskOuter.left)-($taskClient.right-$taskClient.left)
$taskFrameHeight=($taskOuter.bottom-$taskOuter.top)-($taskClient.bottom-$taskClient.top)
if($Width -gt 0 -and $Height -gt 0){
 if($Width -lt 392 -or $Height -lt 570){throw '验收缩放不可低于已约定最小尺寸'}
 [FloatboardWindowProbe]::SetWindowPos($taskHandle,[IntPtr]::Zero,0,0,[int]($Width*$taskScale+$taskFrameWidth),[int]($Height*$taskScale+$taskFrameHeight),0x16)|Out-Null
 [FloatboardWindowProbe]::GetClientRect($taskHandle,[ref]$taskClient)|Out-Null
}
$taskLimits=[FloatboardWindowProbe]::Limits($taskHandle)
$taskEx=[FloatboardWindowProbe]::GetWindowLong($taskHandle,-20)
$taskResult=[pscustomobject]@{width=($taskClient.right-$taskClient.left)/$taskScale;height=($taskClient.bottom-$taskClient.top)/$taskScale;minWidth=($taskLimits.minTrack.x-$taskFrameWidth)/$taskScale;minHeight=($taskLimits.minTrack.y-$taskFrameHeight)/$taskScale;resizable=([FloatboardWindowProbe]::GetWindowLong($taskHandle,-16)-band 0x40000)-ne 0;topmost=($taskEx-band 8)-ne 0;toolWindow=($taskEx-band 0x80)-ne 0;appWindow=($taskEx-band 0x40000)-ne 0;visible=[FloatboardWindowProbe]::IsWindowVisible($taskHandle);minimized=[FloatboardWindowProbe]::IsIconic($taskHandle);scale=$taskScale;foreground=[FloatboardWindowProbe]::GetForegroundWindow().ToInt64();handle=$taskHandle.ToInt64();left=$taskOuter.left;top=$taskOuter.top}
if($OutputPath){$taskResult|ConvertTo-Json|Set-Content -LiteralPath $OutputPath -Encoding utf8}
$taskResult|ConvertTo-Json -Compress
