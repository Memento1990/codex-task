# 两个入口共用分发目录；Reporter 不加载 Qt。
from pathlib import Path
import os
import sys

root = Path(SPECPATH)
conda_bin = Path(sys.prefix) / 'Library' / 'bin'
os.environ['PATH'] = str(conda_bin) + os.pathsep + os.environ.get('PATH', '')
native = [(str(conda_bin / name), '.') for name in ['ffi.dll', 'libmpdec-4.dll', 'sqlite3.dll'] if (conda_bin / name).is_file()]
qt_native = [(str(conda_bin / name), '.') for name in ['Qt6Core.dll', 'Qt6Gui.dll', 'Qt6Widgets.dll', 'Qt6Network.dll'] if (conda_bin / name).is_file()]
gui = Analysis([str(root / 'main.py')], pathex=[str(root)], binaries=native + qt_native, datas=[], hiddenimports=[], hookspath=[], hooksconfig={}, runtime_hooks=[], excludes=['PyQt5', 'PyQt6', 'tkinter', 'numpy', 'pandas', 'matplotlib', 'scipy', 'IPython'])
gui_pyz = PYZ(gui.pure)
gui_exe = EXE(gui_pyz, gui.scripts, [], exclude_binaries=True, name='CodexFloatboard', debug=False, bootloader_ignore_signals=False, strip=False, upx=False, console=False, icon=str(root / 'assets' / 'floatboard.ico'))
reporter = Analysis([str(root / 'reporter.py')], pathex=[str(root)], binaries=native, datas=[], hiddenimports=[], hookspath=[], hooksconfig={}, runtime_hooks=[], excludes=['PySide6', 'PyQt5', 'PyQt6', 'tkinter', 'numpy', 'pandas', 'matplotlib', 'scipy', 'IPython'])
reporter_pyz = PYZ(reporter.pure)
reporter_exe = EXE(reporter_pyz, reporter.scripts, [], exclude_binaries=True, name='FloatboardReporter', debug=False, bootloader_ignore_signals=False, strip=False, upx=False, console=True, icon=str(root / 'assets' / 'floatboard.ico'))
distribution = COLLECT(gui_exe, reporter_exe, gui.binaries, gui.datas, reporter.binaries, reporter.datas, strip=False, upx=False, name='CodexFloatboard')
