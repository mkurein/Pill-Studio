//! The path from reset to running tasks: the steps of startup_stm32*.s,
//! main() and the FreeRTOS port, with their addresses in this build, and the
//! reset causes from RCC_CSR.

use super::elf::Elf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Done by the core itself, before the first instruction.
    Core,
    /// startup_stm32*.s, still on the internal RC oscillator.
    Startup,
    /// main() before the scheduler.
    Main,
    /// FreeRTOS start and the first task.
    Rtos,
    Task,
}

impl Stage {
    pub fn ru(self) -> &'static str {
        match self {
            Stage::Core => "ядро",
            Stage::Startup => "startup",
            Stage::Main => "main",
            Stage::Rtos => "FreeRTOS",
            Stage::Task => "задачи",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub stage: Stage,
    /// Symbol the step starts at; "" for what the core does itself.
    pub sym: String,
    pub title: String,
    pub what: String,
    /// Address of `sym` in this ELF (Thumb bit cleared); None when the
    /// compiler inlined it or the firmware has no such function.
    pub addr: Option<u32>,
}

/// Calls that say nothing about the boot path.
const NOISE: [&str; 9] = ["main", "log_printf", "printf", "Error_Handler", "memset", "memcpy", "strlen", "fram_kind", "HAL_Delay"];

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |e| &rest[i + e + 2..]);
    }
    out.push_str(rest);
    out.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

/// Body of `int main(...)` without comments.
fn main_body(src: &str) -> Option<String> {
    let clean = strip_comments(src);
    let start = clean.find("int main(")?;
    let open = start + clean[start..].find('{')?;
    let mut depth = 0;
    for (i, c) in clean[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(clean[open..open + i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// `name(` occurrences in order, first time only.
fn calls(body: &str) -> Vec<String> {
    let b = body.as_bytes();
    let mut out: Vec<String> = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let name = &body[s..i];
            let mut k = i;
            while k < b.len() && b[k] == b' ' {
                k += 1;
            }
            if b.get(k) == Some(&b'(') && !out.iter().any(|x| x == name) {
                out.push(name.to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Functions started as tasks: first argument of `osThreadNew(` / `xTaskCreate(`.
fn task_entries(body: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for key in ["osThreadNew(", "xTaskCreate("] {
        for (i, _) in body.match_indices(key) {
            let name: String = body[i + key.len()..].trim_start().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

fn known(sym: &str) -> (Stage, &'static str) {
    match sym {
        "diag_boot" => (Stage::Main, "Нет метки DIAG_MAGIC в g_diag (было выключено питание) — запись обнуляется. boot_count +1, причина сброса сохраняется, запись о прошлом сбое остаётся."),
        "HAL_Init" => (Stage::Main, "Группировка приоритетов NVIC, таймер тиков HAL, HAL_MspInit."),
        "SystemClock_Config" => (Stage::Main, "Кварц HSE и PLL, задержка Flash, делители шин; такт переключается с HSI на PLL. Кварц не запустился — Error_Handler."),
        "fram_init" | "fram_cfg_setup" | "fram_probe" => (Stage::Main, "Внешняя SPI-память: настройка и проверка, что микросхема отвечает."),
        "log_init" => (Stage::Main, "Лог-UART готов к выводу из задач."),
        "uart1_start_rx" => (Stage::Main, "Включается приём USART1 по прерыванию — только после создания очереди."),
        "osKernelInitialize" => (Stage::Rtos, "Ядро FreeRTOS готовится, но ещё не работает."),
        "osThreadNew" | "xTaskCreate" => (Stage::Rtos, "Создание задач: стек и блок управления каждой выделяются из кучи FreeRTOS (ucHeap)."),
        "osKernelStart" => (Stage::Rtos, "Запуск планировщика. Из этой функции main уже не возвращается."),
        _ => (Stage::Main, "Вызов из main() этого проекта."),
    }
}

/// Steps in the order they run. Addresses and sizes come from this ELF; the
/// calls of `main()` and the tasks come from `main_c` (the project's
/// Core/Src/main.c), so a new call or task shows up by itself. Without the
/// source the usual CubeMX sequence is used.
pub fn steps(elf: &Elf, main_c: Option<&str>) -> Vec<Step> {
    let l = |n: &str| elf.labels.get(n).copied();
    let a = |n: &str| l(n).map(|v| v & !1);
    let h = |v: Option<u32>| v.map(|v| format!("0x{v:08X}")).unwrap_or_else(|| "?".into());
    let span = |lo: &str, hi: &str| match (l(lo), l(hi)) {
        (Some(x), Some(y)) if y >= x => (y - x).to_string(),
        _ => "?".into(),
    };
    let mut v = vec![Step {
        stage: Stage::Core,
        sym: String::new(),
        title: "Сброс: SP и PC из таблицы векторов".into(),
        what: format!(
            "Ядро само читает из начала Flash SP = {} и PC = {} (Reset_Handler). Периферия — в значениях по сбросу, такт HSI, прерывания выключены.",
            h(elf.vectors.first().copied()),
            h(elf.vectors.get(1).copied())
        ),
        addr: None,
    }];
    let mut add = |stage: Stage, sym: &str, title: &str, what: String| {
        v.push(Step { stage, sym: sym.into(), title: title.into(), what, addr: a(sym) });
    };
    add(Stage::Startup, "Reset_Handler", "Reset_Handler", "Первая инструкция прошивки (startup_stm32*.s).".into());
    add(Stage::Startup, "SystemInit", "SystemInit", "На F4 разрешает FPU, задаёт VTOR. Тактирование не трогает.".into());
    add(
        Stage::Startup,
        "LoopCopyDataInit",
        "Копирование .data",
        format!("{} Б начальных значений из Flash ({}) в RAM ({}).", span("_sdata", "_edata"), h(l("_sidata")), h(l("_sdata"))),
    );
    add(
        Stage::Startup,
        "LoopFillZerobss",
        "Обнуление .bss",
        format!("{} Б с {} заполняются нулями. g_diag (.noinit) лежит дальше и не затирается.", span("_sbss", "_ebss"), h(l("_sbss"))),
    );
    add(Stage::Main, "main", "main", "Начало программы на C: читается причина сброса (RCC_CSR), флаги стираются.".into());
    let body = main_c.and_then(main_body);
    let from_src: Vec<String> = match &body {
        Some(b) => calls(b).into_iter().filter(|n| !NOISE.contains(&n.as_str()) && a(n).is_some()).collect(),
        None => ["diag_boot", "HAL_Init", "SystemClock_Config", "MX_GPIO_Init", "osKernelInitialize", "osThreadNew", "osKernelStart"].map(String::from).to_vec(),
    };
    let mut mx: Vec<&str> = vec![];
    for n in &from_src {
        if n.starts_with("MX_") {
            // all CubeMX init calls are one step, at the first of them
            if mx.is_empty() {
                add(Stage::Main, n, "Инициализация периферии", String::new());
            }
            mx.push(n);
            continue;
        }
        let (stage, what) = known(n);
        add(stage, n, n, what.into());
        if n == "osKernelStart" {
            add(Stage::Rtos, "xPortStartScheduler", "xPortStartScheduler", "PendSV и SysTick получают приоритет 15, запускается SysTick на 1 мс.".into());
            add(Stage::Rtos, "SVC_Handler", "SVC_Handler", "Исключение SVCall: восстанавливаются регистры первой задачи, ядро переходит на её стек (PSP).".into());
        }
    }
    if let Some(s) = v.iter_mut().find(|s| s.title == "Инициализация периферии") {
        s.what = mx.join(", ");
    }
    let tasks: Vec<String> = match &body {
        Some(b) => task_entries(b),
        None => ["Uart1Task", "LedTask", "StartDefaultTask"].map(String::from).to_vec(),
    };
    let mut add = |stage: Stage, sym: &str, title: &str, what: String| {
        v.push(Step { stage, sym: sym.into(), title: title.into(), what, addr: a(sym) });
    };
    for t in &tasks {
        add(Stage::Task, t, t, "Задача запущена: первая инструкция её функции.".into());
    }
    v
}

/// One step reached during a live boot trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub addr: u32,
    /// Position in the order the steps were actually reached, from 0.
    pub order: usize,
    /// Core clock cycles since reset (DWT_CYCCNT; not counted while halted).
    pub cycles: u32,
}

/// Reset flags of RCC_CSR (same bits on F1 and F4; BOR only on F4).
pub const RESET_FLAGS: [(u32, &str, &str); 7] = [
    (27, "POR/PDR", "включение питания или его провал ниже порога PDR"),
    (25, "BOR", "просадка питания ниже порога BOR (только F4)"),
    (26, "PIN", "вывод NRST: кнопка, ST-Link; ставится и при любом другом сбросе"),
    (28, "SFT", "программный сброс: NVIC_SystemReset, команда RESET, reset по SWD"),
    (29, "IWDG", "независимый сторожевой таймер (в прошивке не включён)"),
    (30, "WWDG", "оконный сторожевой таймер (в прошивке не включён)"),
    (31, "LPWR", "недопустимый вход в режим низкого потребления"),
];

/// The one cause that matters, most specific first.
pub fn main_cause(csr: u32) -> &'static str {
    let b = |n: u32| csr & (1 << n) != 0;
    if b(27) {
        "включение питания"
    } else if b(25) {
        "просадка питания (BOR)"
    } else if b(29) {
        "сторожевой таймер IWDG"
    } else if b(30) {
        "сторожевой таймер WWDG"
    } else if b(31) {
        "сбой режима низкого потребления"
    } else if b(28) {
        "программный сброс"
    } else if b(26) {
        "вывод NRST"
    } else {
        "флагов нет"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN_C: &str = "\
void SystemClock_Config(void);
int main(void) {
  uint32_t c = RCC->CSR; /* cause, not a call( */
  diag_boot(c);
  HAL_Init();
  // old_init();
  SystemClock_Config();
  MX_GPIO_Init();
  MX_SPI1_Init();
  log_printf(\"BOOT\\n\");
  my_new_driver_init();
  if (q == NULL) { Error_Handler(); }
  osKernelInitialize();
  h1 = osThreadNew(StartDefaultTask, NULL, &a1);
  h2 = osThreadNew(BlinkTask, NULL, &a2);
  osKernelStart();
  while (1) {}
}
void after(void) { never(); }
";

    fn elf_with(names: &[&str]) -> Elf {
        let mut elf = Elf { vectors: vec![0x2002_0000, 0x0800_550D], ..Default::default() };
        for (i, n) in names.iter().enumerate() {
            elf.labels.insert(n.to_string(), 0x0800_1001 + 0x100 * i as u32);
        }
        for (n, v) in [("_sdata", 0x2000_0000u32), ("_edata", 0x2000_0060), ("_sidata", 0x0800_65EC), ("_sbss", 0x2000_0060), ("_ebss", 0x2000_58A4)] {
            elf.labels.insert(n.into(), v);
        }
        elf
    }

    #[test]
    fn steps_take_addresses_and_sizes_from_the_elf() {
        let s = steps(&elf_with(&["Reset_Handler", "main"]), None);
        assert!(s[0].what.contains("0x20020000") && s[0].what.contains("0x0800550D"));
        let get = |n: &str| s.iter().find(|x| x.sym == n).unwrap();
        assert_eq!(get("Reset_Handler").addr, Some(0x0800_1000));
        assert_eq!(get("SystemInit").addr, None);
        assert!(get("LoopCopyDataInit").what.starts_with("96 Б"));
        assert!(get("LoopFillZerobss").what.starts_with("22596 Б"));
        assert!(s.iter().any(|x| x.sym == "LedTask"));
    }

    #[test]
    fn main_calls_and_tasks_follow_the_source() {
        let elf = elf_with(&[
            "main", "diag_boot", "HAL_Init", "SystemClock_Config", "MX_GPIO_Init", "MX_SPI1_Init", "log_printf", "my_new_driver_init",
            "Error_Handler", "osKernelInitialize", "osThreadNew", "osKernelStart", "StartDefaultTask", "BlinkTask", "old_init", "never",
        ]);
        let s = steps(&elf, Some(MAIN_C));
        let names: Vec<&str> = s.iter().filter(|x| matches!(x.stage, Stage::Main | Stage::Rtos | Stage::Task)).map(|x| x.sym.as_str()).collect();
        assert_eq!(
            names,
            [
                "main", "diag_boot", "HAL_Init", "SystemClock_Config", "MX_GPIO_Init", "my_new_driver_init", "osKernelInitialize", "osThreadNew",
                "osKernelStart", "xPortStartScheduler", "SVC_Handler", "StartDefaultTask", "BlinkTask",
            ]
        );
        let mx = s.iter().find(|x| x.sym == "MX_GPIO_Init").unwrap();
        assert_eq!((mx.title.as_str(), mx.what.as_str()), ("Инициализация периферии", "MX_GPIO_Init, MX_SPI1_Init"));
    }

    #[test]
    fn reset_cause_prefers_the_specific_flag() {
        assert_eq!(main_cause(1 << 27 | 1 << 26), "включение питания");
        assert_eq!(main_cause(1 << 28 | 1 << 26), "программный сброс");
        assert_eq!(main_cause(1 << 26), "вывод NRST");
        assert_eq!(main_cause(1 << 29 | 1 << 26), "сторожевой таймер IWDG");
        assert_eq!(main_cause(0), "флагов нет");
    }
}
