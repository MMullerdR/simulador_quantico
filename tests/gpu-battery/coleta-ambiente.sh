#!/usr/bin/env bash
# Coleta a identificação da máquina para o registro de uma bateria de testes
# (docs/11-bateria-testes-gpu.md). Só lê informação do sistema; não instala nem
# altera nada. Saída em Markdown, para colar em RESULTADO-TEMPLATE.md.
# Funciona em Linux, macOS e Windows (Git Bash/WSL); o que não existir na
# máquina aparece como "(indisponível)".
set -u

cmd_or(){ # cmd_or <comando> [args...]: roda se existir, senão avisa
	if command -v "$1" >/dev/null 2>&1; then "$@" 2>&1; else echo "(indisponível: $1)"; fi
}

echo "## Ambiente"
echo
echo "- Data: $(date '+%Y-%m-%d %H:%M:%S %z')"
echo "- Commit do repositório: $(git rev-parse --short HEAD 2>/dev/null || echo '(fora de um repositório git)') ($(git describe --tags --always 2>/dev/null || echo '-'))"
echo "- Kernel/sistema: $(uname -srm 2>/dev/null)"

if [ -r /etc/os-release ]; then
	echo "- Distribuição: $(. /etc/os-release; echo "$PRETTY_NAME")"
elif command -v sw_vers >/dev/null 2>&1; then
	echo "- macOS: $(sw_vers -productName) $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
elif command -v cmd.exe >/dev/null 2>&1; then
	echo "- Windows: $(cmd.exe /c ver 2>/dev/null </dev/null | tr -d '\r' | grep -E '\[' | head -1)"
fi

echo
echo "### CPU e memória"
echo
echo '```'
if [ -r /proc/cpuinfo ]; then
	grep -m1 'model name' /proc/cpuinfo
	echo "threads lógicos: $(nproc 2>/dev/null || grep -c '^processor' /proc/cpuinfo)"
	grep -m1 MemTotal /proc/meminfo
elif command -v sysctl >/dev/null 2>&1; then
	sysctl -n machdep.cpu.brand_string 2>/dev/null
	echo "threads lógicos: $(sysctl -n hw.logicalcpu 2>/dev/null)"
	echo "memória (bytes): $(sysctl -n hw.memsize 2>/dev/null)"
elif command -v powershell.exe >/dev/null 2>&1; then
	powershell.exe -NoProfile -Command "Get-CimInstance Win32_Processor | Select-Object Name,NumberOfLogicalProcessors | Format-List; (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory" 2>&1 | tr -d '\r' | sed '/^$/d'
else
	echo "(indisponível)"
fi
echo '```'

echo
echo "### GPU(s)"
echo
echo '```'
if command -v nvidia-smi >/dev/null 2>&1; then
	nvidia-smi --query-gpu=name,driver_version,memory.total,memory.free --format=csv 2>&1
fi
if command -v lspci >/dev/null 2>&1; then
	lspci 2>/dev/null | grep -iE 'vga|3d|display'
fi
if command -v system_profiler >/dev/null 2>&1; then
	system_profiler SPDisplaysDataType 2>/dev/null | grep -E 'Chipset Model|VRAM|Metal|Vendor|Cores'
fi
if command -v powershell.exe >/dev/null 2>&1; then
	powershell.exe -NoProfile -Command "Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,AdapterRAM | Format-List" 2>&1 | tr -d '\r' | sed '/^$/d'
fi
echo '```'

echo
echo "### APIs gráficas e toolchains"
echo
echo '```'
if command -v vulkaninfo >/dev/null 2>&1; then
	vulkaninfo --summary 2>&1 | grep -E 'apiVersion|deviceName|driverName|driverInfo|deviceType' | head -20
else
	echo "vulkaninfo: (indisponível)"
fi
echo "nvcc:  $(cmd_or nvcc --version | grep -iE 'release|indisp')"
echo "g++:   $(cmd_or g++ --version | head -1)"
echo "rustc: $(cmd_or rustc --version)"
echo "cargo: $(cmd_or cargo --version)"
echo '```'
