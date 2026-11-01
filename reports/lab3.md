### 功能

​	迁移了  `sys_get_time` `sys_mmap` `sys_munmap`  三个系统调用，代码没有很大区别，主要是从任务管理器到进程管理器转换后相应的 API 发生变化。

​	实现了 `sys_spawn` 系统调用，根据目标程序的地址来创建一个子进程。我写代码下来的感觉是将 fork 和 exec 结合了起来，用 fork 的逻辑把子进程和父进程联系起来，用 exec 的逻辑把创建子进程的空间以及维护 TCB 这种信息。

​	实现了 stride 调度算法，将原本 FIFO 的简单调度算法换成了根据进程优先级来调度进程，将任务管理器内部的任务管理结构换成了一个最小堆，每次弹出 stride 最小的进程运行，并更新 stride。BIG_STRIDE 设置为了 $$ 2 ^ {64}$$ 。设置了 stride 的溢出处理，如果有溢出，将当前所有进程的 stride 减少最小进程的 stride 数值，维护一个相对大小关系。

---

### 简答作业

Q : 实际情况是轮到 p1 执行吗？为什么？

A :  不是，$$p2.stride + 10 = 4$$ （溢出了），每次找 stride 最小的进程运行，所以下一次变成了 p2 运行。



Q：为什么？尝试简单说明（不要求严格证明）

A： 尝试使用数学归纳法：
	初始状态所有 $$stride = 0$$，$$STRIDE\_MAX-STRIDE\_MIN = 0 <= BigStride / 2$$，显然成立
	每次取最小的 $$stride$$ 去加上 $$pass_i$$，$$pass_i = BigStride / priority_i$$  ，因为 $$priority >= 2$$，所以 $$pass <= BigStride / 2$$，
	如果到当前状态都满足 $$STRIDE\_MAX_{i}-STRIDE\_MIN_{i} <= BigStride / 2$$，
	本次取 $$STRIDE\_MIN$$ 加上对应的 $$pass_i$$，
	那 $$STRIDE\_MAX-(STRIDE\_MIN+pass_i) <= BigStride / 2$$，
	此时新的最小 $$stride$$ 满足 $$STRIDE\_MIN_{i}<=STRIDE\_MIN_{i+1}<= (STRIDE\_MIN+pass_i)$$，
	所以 $$STRIDE\_MAX_{i+1}-STRIDE\_MIN_{i+1} <= BigStride / 2$$，
	因为初始状态满足，经过归纳，得证。



Q： 考虑溢出的情况下，为 Stride 设计特别的比较器

A：

```rust
struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let diff= BIG_STRIDE / 2;
        if self.0 <= diff {
            if other.0 > self.0 && other.0 <= self.0 + diff {
                return Some(Ordering::Less);
            }else {
                return Some(Ordering::Greater);
            }
        }else {
            if other.0 >= self.0 - diff && other.0 < self.0 {
                return Some(Ordering::Greater);
            }else {
                return Some(Ordering::Less);
            }
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}
```

基于上面的理论，如果以 self 为基准，other 一定落在 [self - BIG_STRIDE / 2 , self + BIG_STRIDE / 2] 这个区间内。

为了避免有溢出问题，根据 self 和 BIG_STRIDE / 2 的大小关系，我们判断 other 是落在左半区间还是右半区间，来确定排序关系。

---

### 荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   > 与 chatgpt 交流
   >
   > 在我写完整体代码后，让 gpt 进行 Debug。
   > 发现我在 `insert_framed_area` 这个函数里，判断页表是否已经映射没有判断当前页是不是 valid，本来我认为只要能找到 pte 就是有映射了，所以后面内核栈都没有成功建立，加上建立内核栈那里我没有错误处理，自己 Debug 也不知道从何下手。
   >
   > 在 gpt 帮助下优化了一下我的 stride 的溢出处理。
   >
   > 同时在了解代码架构以及最后写问答题的过程中都有和 gpt 的交流，主要是一些设计理念，很多时候我想让我的代码更贴合操作系统，代码风格更 rusty 一点。

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   > 暂无

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

---

### 可能需要改进的地方

`task.rs` 里的 `exec` 函数，加载新的程序后没有更新自身的 `program_brk` 和 `heap_bottom`，还在沿用父进程的数值。
