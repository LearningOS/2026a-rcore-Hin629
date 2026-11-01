### 功能

​	实现了 `sys_trace` 这个系统调用。

​	对于（读/写）内存操作，在 `unsafe` 的作用域下将地址转换成裸指针，进行解引用与内存读写操作。

​	对于查询系统调用次数这个功能，在 `TaskManagerInner` 里面新增系统调用计数器，类型是 `Vec<BTreeMap<usize,usize>>` ，外层 `Vec` 以 `Task ID` 为索引，隔离不同任务间的统计数据，对于记录 `id` 这个任务的 `BTreeMap` ，根据系统调用号 `syscall_id` ，获取 对应键值对，键是 `syscall_id` 的数值，值是调用的次数。在每一次进入 `Trap` 的处理系统调用时将对应的系统调用记录 + 1。

---

### 简答作业

**1.**

RustSBI 版本由内核启动的时候显示如下

```
RustSBI version 0.4.1
```

`ch2b_bad_address.rs` 里尝试去写入 `0x0` 这个非法内存，`kernel` 处理报错，kill 掉程序

```
[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003a4, kernel killed it.
```

`ch2b_bad_instructions.rs` 尝试使用 `sret` 这个只能在 S 态的特权指令，进入 `IllegalInstruction` Trap，内核 kill 掉程序

```
[kernel] IllegalInstruction in application, kernel killed it.
```

`ch2b_bad_register.rs` 尝试访问 S 态的特权寄存器 `sstatus` ，也是进入 `IllegalInstruction` Trap，内核 kill 掉程序

```
[kernel] IllegalInstruction in application, kernel killed it.
```

**2.**

​	**1.**

刚进入 `__restore` 时，`sp` 表示当前应用的 `TrapContext` 的初始地址， `TrapContext` 里面存放了应用在进入 `Trap` 之前在用户态正常运行时的寄存器信息。

 `__restore` 主要解决的是任务如何从内核态返回用户态并能正常运行。

所以第一种调用方式就是普通 Trap 返回用户态的时候用：
	由于 `stvec::write(__alltraps as usize, TrapMode::Direct);` ，用户态在 Trap 后会从 `__alltraps ` 开始执行，在内核栈分配一块空间保存各种用户寄存器信息，然后 `call trap_handler` 进入内核的 Trap 处理代码，因为是调用函数，这个时候的 `ra` 寄存器就被设置为 `call trap_handler` 的下一行代码，也就是  `__restore` 部分代码的开始，负责还原用户态的寄存器并释放前面 `__alltraps ` 分配的内核栈。

第二种调用方式是在内核开始运行第一个程序的时候：
	当前代码实现了根据时间片的多任务切换，在系统启动的时候，给所有任务的 `TaskContext` 初始化，内部的 `ra` 设置为  `__restore` ，在依靠 `__switch` 切换 Task 后，函数结束就直接去到 `__restore` 的代码里，根据初始化的数据还原寄存器的内容，并进入用户态了。

​	**2.**

处理了 `sstatus`, `sepc`,`sscratch` 这三个寄存器，从内核栈中恢复了这三个寄存器的数值。

 `sstatus` 内有保存 CPU 特权级（SPP）和中断相关状态（SPIE），决定了在这条 Trap 返回后 CPU进入什么模式（U/S），以及当时的中断使能等信息。

 `sepc `保存了 Trap 返回后，下一条指令应该去执行哪个地址的指令，决定了 `sret` 后 CPU 应该从哪条指令开始执行。

`sscratch` 读取了`2*8(sp)` ，也就是原来保存的用户态程序的 `sp` 地址，后面还有一条指令 `csrrw sp, sscratch, sp` 把这个寄存器和当前内核态的 `sp` 进行了交换，最终就是 `sp` 指向保存的原先用户态程序的 `sp`，`sscratch` 指向内核态的栈地址，之后再进入 `Trap` 态的时候，`sp` 会被设置为 `sscratch` 。

​	**3**

`x2` 就是 `sp` 指针，后面还有寄存器需要根据内核态栈的 `sp` 进行恢复，而且内核栈也需要释放，所以不能在这行代码就恢复 `x2`。后面的 `csrrw sp, sscratch, sp` 就是在恢复 `sp`。

`x4` 经过查询知道是 ``tp（Thread Pointer）` 指针，当前不会用到，所以不恢复。

​	**4**

最终就是 `sp` 指向保存的原先用户态程序的 `sp`，`sscratch` 指向现在内核态的栈地址。

​	**5**

`sret` 发生状态切换。会根据 `sstatus` 里的 `SPP` 信息决定要返回什么态，这里因为初始化设置过 or  本来就是从用户态 Trap 过来的，所以 `sret` 就会返回用户态。

​	**6**

这条指令后，`sp` 是内核栈地址，`sscratch` 是原先的 `sp` 也就是用户态的栈地址。

​	**7**

触发 Trap 异常后，硬件就会自动从 U 态切到 S 态，比如用户态执行 `ecall` 系统调用指令，或者是有时钟中断或者是非法指令异常这种。

---

### 荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   > 主要是与 chatgpt 交流
   >
   > 在理解整个内核的过程中，很多问题都会问到 ai 吧，比如做这个问答题的时候，开始对 alltraps 和  restore 以及 Trap，Task 这些上下文切换的设计思路不是很能理解，感觉很绕。这里的回答是问完熟悉整个过程后之后根据我目前的理解写的。
   >
   > 以及就是本章节要实现的功能，我开始想法是在每个 TaskControlBlock 里去建一个数组记录，开始打算设置每个都有 512 个元素的，因为系统调用号很大，但是这样很占内存，每个 TaskControlBlock  不是全局变量所以要分配到栈上，当前内核栈又比较小，很容易就栈溢出了，而且也没必要这么存。
   > 然后就想到用 map 存键值对，开始也就是想在每个 TaskControlBlock  里存一个 map，但是他实现了 \#[derive(Copy, Clone)]，而 map 和 Copy 是冲突的，所以最后这个方法也没用到。最后还是按现在的方法把计数器放在了全局的 TaskManagerInner 上。这整个过程很多思路的解答是建立在 ai 上的。

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   > 暂无

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

---

### 可能需要改进的地方

`rCore-Tutorial` 里面用的 `qemu` 版本是 7.0.0，和最新的 RustSBI 有点不兼容，我以前是跟着 `Tutorial` 下的 7.0.0，所以刚开始做这个任务的时候也是调用的 7.0.0，然后就发现会卡在下面：

```
[RustSBI] INFO  - 7     TOR        RWX             0xfffffffffffffffc
[RustSBI] INFO  - Boot HART ID                  : 0
```

然后我还不会调，就让 gpt 找了下原因，发现是 0.4.1 的 RustSBI 会在启动的时候运行指令 ` csrr a2, mcountinhibit`，这个在 `qemu-7.0.0` 还不支持，日志写了

```
epc: 0x8001349e
desc: illegal_instruction
```

在这个指令后就异常了，也就在 SBI 启动阶段就卡住了。

感觉可以尝试更新一下 Tutorial 文档。
