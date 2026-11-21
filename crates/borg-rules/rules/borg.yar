rule reverse_shell
{
    meta:
        description = "A shell wired to a remote host: an interactive shell over /dev/tcp, nc or socat running a shell, or a shell spawned on a pseudo-terminal"
        severity = "alert"
        false_positives = "Penetration testing tools, security training material, and other tools' rule sets"
    strings:
        $dev_tcp = /(ba|z|k|da)?sh\x20+-i\x20*[0-9]?[<>&]{1,3}\x20*\x2fdev\x2f(tcp|udp)\x2f/
        $nc = /\bn(c|cat|etcat)\x20[^\n]{0,80}-[ec]\x20*\x2fbin\x2f(ba|z|da)?sh\b/
        $socat = /socat\x20[^\n]{0,80}exec:[^\n]{0,40}(ba|z|da)?sh\b/ nocase
        $fifo = /mkfifo\x20[^\n]{1,80}\|\x20*\x2fbin\x2f(ba|z|da)?sh\x20+-i/
        $pty = /pty\.spawn\(\x20*["']\x2fbin\x2f(ba|z|da)?sh/
    condition:
        any of them
}

rule download_and_run
{
    meta:
        description = "Downloads a script and pipes it straight into a shell"
        severity = "notice"
        false_positives = "Installers that projects publish this way, such as package managers and language toolchains"
    strings:
        $pipe = /\b(curl|wget)\x20[^\n|;]{1,300}\|\x20*(sudo\x20+)?(ba|z|da)?sh\b/
    condition:
        $pipe
}

rule decode_and_run
{
    meta:
        description = "Decodes base64 and runs the result as a shell command or Python code"
        severity = "warning"
        false_positives = "Installers and deployment scripts that carry an embedded payload"
    strings:
        $shell = /base64\x20+(-d|--decode|-D)\b[^\n|]{0,40}\|\x20*(sudo\x20+)?(ba|z|da)?sh\b/
        $python = /exec\(\x20*(base64\.)?b64decode\(/
    condition:
        any of them
}

rule crypto_miner
{
    meta:
        description = "Cryptocurrency mining: a mining pool address, or the settings of a common miner"
        severity = "warning"
        false_positives = "Mining software you installed yourself"
    strings:
        $stratum = { 73 74 72 61 74 75 6D ( 2B | 32 2B ) ( 74 63 70 | 73 73 6C | 74 6C 73 ) 3A 2F 2F }
        $donate = { 64 6F 6E 61 74 65 2D 6C 65 76 65 6C }
        $randomx = { 72 61 6E 64 6F 6D 78 }
    condition:
        $stratum or ($donate and $randomx)
}

rule upx_packed
{
    meta:
        description = "An executable packed with UPX, which unpacks itself in memory when it runs"
        severity = "notice"
        false_positives = "Ordinary programs packed to make them smaller"
    strings:
        $upx = { 55 50 58 21 }
    condition:
        (uint32(0) == 0x464C457F or uint32(0) == 0xFEEDFACF or uint32(0) == 0xFEEDFACE or uint16(0) == 0x5A4D)
        and $upx in (0..4096)
}

rule execve_shellcode
{
    meta:
        description = "The shell path that exploit code builds on the stack before it starts a shell"
        severity = "warning"
        false_positives = "Exploit development tools, security courses, and tests of debuggers and emulators"
    strings:
        $bin_sh = { 2F 62 69 6E 2F 2F 73 68 }
        $sh_bin = { 2F 2F 62 69 6E 2F 73 68 }
        $push = { 68 2F 2F 73 68 68 2F 62 69 6E }
    condition:
        any of them
}

rule ssh_key_planted
{
    meta:
        description = "Adds a public key to an authorized_keys file, which lets its owner log in over SSH"
        severity = "warning"
        false_positives = "Provisioning scripts such as cloud-init, Ansible and image builders"
    strings:
        $append = /(>>|tee\x20+-a)\x20*[^\n]{0,60}\.ssh\x2fauthorized_keys/
        $key = /ssh-(rsa|ed25519|dss)\x20AAAA/
    condition:
        $append and $key
}

rule reboot_job_fetch
{
    meta:
        description = "A cron job that runs at every boot and downloads, decodes or runs something from a temporary folder"
        severity = "warning"
        false_positives = "Boot jobs administrators set up on purpose"
    strings:
        $reboot = /@reboot\x20[^\n]{0,200}(curl|wget|base64\x20+-d|\x2ftmp\x2f|\x2fdev\x2fshm\x2f|nohup)/
    condition:
        $reboot
}

rule password_prompt
{
    meta:
        description = "An AppleScript dialog that asks for a password with the answer hidden, as macOS stealers do"
        severity = "warning"
        false_positives = "Administration scripts that ask for a password themselves instead of using sudo"
    strings:
        $dialog = /display\x20+dialog[^\n]{0,400}hidden\x20+answer/ nocase
    condition:
        $dialog
}

rule launch_agent_from_temp
{
    meta:
        description = "A macOS launch agent or daemon that starts a program from a temporary or shared folder at login"
        severity = "warning"
        false_positives = "Installers that run a helper from a temporary folder once"
    strings:
        $program = "<key>ProgramArguments</key>"
        $run = "<key>RunAtLoad</key>"
        $temp = /<string>\x2f(private\x2f)?(tmp|var\x2ftmp|Users\x2fShared)\x2f/
    condition:
        $program and $run and $temp
}

rule preload_write
{
    meta:
        description = "Writes to /etc/ld.so.preload, which loads a library into every program on a Linux system"
        severity = "warning"
        false_positives = "Memory allocators and profilers installed system-wide on purpose"
    strings:
        $write = /(>>?|tee(\x20+-a)?)\x20*\x2fetc\x2fld\.so\.preload/
    condition:
        $write
}

rule history_wiping
{
    meta:
        description = "Turns off the shell history or empties system logs, which hides what was done"
        severity = "notice"
        false_positives = "Privacy settings in shell profiles and log cleanup scripts"
    strings:
        $unset = /unset\x20+HISTFILE\b/
        $null = /HISTFILE=\x2fdev\x2fnull/
        $clear = /history\x20+-c\b/
        $logs = /rm\x20+-(rf|fr|r|f)\x20+\x2fvar\x2flog\b/
        $login_records = /[>:]\x20*\x2fvar\x2flog\x2f(wtmp|btmp|lastlog|auth\.log|secure)\b/
    condition:
        any of them
}

rule agent_unattended
{
    meta:
        description = "Runs an AI coding agent with its approvals turned off, as the Nx supply-chain attack did to search machines for secrets"
        severity = "warning"
        false_positives = "Your own aliases, scripts and CI jobs that run an agent unattended"
    strings:
        $claude = "\x2d\x2ddangerously-skip-permissions"
        $codex = "\x2d\x2ddangerously-bypass-approvals-and-sandbox"
        $amazon_q = "\x2d\x2dtrust-all-tools"
        $gemini = /\bgemini\b[^\n]{0,200}\x2d\x2dyolo\b/
    condition:
        any of them
}

rule hidden_prompt
{
    meta:
        description = "A run of invisible Unicode tag characters, which people do not see but AI models read: a way to hide instructions in prompts, rule files and web pages"
        severity = "warning"
        false_positives = "Emoji flags use a few such characters; long runs are rare outside prompt injection research"
    strings:
        $tags = /(\xF3\xA0[\x80\x81][\x80-\xBF]){10}/
    condition:
        $tags
}

rule pickle_runs_code
{
    meta:
        description = "A pickle, the format Python and PyTorch save models and data in, that runs a command or code when it is loaded"
        severity = "alert"
        false_positives = "Test data of pickle tools and security research"
    strings:
        $global = /c(os|posix|nt|subprocess|builtins|__builtin__|runpy)\n(system|popen|Popen|call|check_call|check_output|run|exec|eval|_run_code)\n/
        $stack = /\x8c[\x02-\x0b](os|posix|nt|subprocess|builtins|runpy)\x94?\x8c[\x03-\x0c](system|popen|Popen|call|check_call|check_output|run|exec|eval|_run_code)\x94?\x93/
    condition:
        any of them
}
