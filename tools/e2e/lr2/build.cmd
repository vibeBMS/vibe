@echo off
rem Builds lr2probe.exe (32-bit, as LR2 is) with the runner's Visual Studio.
rem   build.cmd OUT.exe
setlocal
for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath`) do set VS=%%i
call "%VS%\VC\Auxiliary\Build\vcvarsall.bat" x86 >nul || exit /b 1
cl /nologo /O2 /W3 /D_CRT_SECURE_NO_WARNINGS "%~dp0probe.c" /Fo"%TEMP%\\" /Fe"%~1" ^
  d3d9.lib dsound.lib dinput8.lib winmm.lib ole32.lib gdi32.lib user32.lib || exit /b 1
