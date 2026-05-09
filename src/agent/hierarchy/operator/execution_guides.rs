//! 分层算子 ReAct：按任务描述关键词选用的执行指导长文案（与 `prompt.rs` 中 `build_execution_guide` 配套）。

pub(super) const DEFAULT: &str = "分析任务需求；在首次调用工具前完成系统提示中的首轮复述（除非用户免去或属于无需工具的极简问答）；再选择合适的工具，逐步执行并验证结果。";

pub(super) const BUILD: &str = r#"这是一个编译/构建任务，请按以下步骤执行：

**步骤 0: 确定工作目录（关键！）**
- 若源码在子目录中（记为 `SRC/`，名称以 `read_dir` 为准），**所有后续命令必须针对该目录**
- **当前工作目录**是固定的，不会因为你执行了 `cd` 而改变
- **执行命令的三种方式**（按推荐顺序）：
  1. **使用 `-C` 选项**（最推荐）：`make -C SRC`
  2. **完整路径作为 command**：`command: "SRC/configure"`, `args: []`
  3. **完整路径在 args 中**：`command: "cp"`, `args: ["SRC/setup/file", "SRC/dest"]`
- **常见错误**：
  - ❌ `command: "./configure"`, `args: ["SRC/configure"]` —— command 与 args 重复了路径
  - ❌ `command: "cp"`, `args: ["setup/file", "dest"]` —— 当前目录下没有 `setup/` 或未在 `SRC/` 下
  - ✅ `command: "cp"`, `args: ["SRC/setup/模板文件", "SRC/Make.custom"]` —— 使用经 `read_dir` 确认存在的相对路径
- **重要**：复制配置、执行 `make` 等操作都要使用完整工作区相对路径，或配合 `make -C SRC`

**步骤 1: 检测构建系统**
- 使用 `read_dir` 查看源码目录结构（注意使用正确的路径）
- 检查是否存在以下构建文件（按优先级）：
  * CMakeLists.txt → 使用 cmake 构建
  * configure 脚本 → 使用 ./configure && make
  * Makefile → 使用 make
  * build.gradle/pom.xml → Java 项目
  * package.json → Node.js 项目

**步骤 2: 检查编译器/工具链**
- 使用 `which` 检查必要的编译器是否存在（gcc/g++, cmake, make 等）
- 如果编译器不存在，报告错误并终止（不要反复尝试不同的 which 组合）

**步骤 3: 执行构建（注意工作目录）**
- CMake 项目：
  0. **若你编写 CMakeLists.txt**：单文件示例用 `add_executable(目标名 main.cpp)`；**勿**在仓库根用 `file(GLOB_RECURSE "*.cpp" …)` 且不排除 `build/`、`CMakeFiles/`（会把 CMake 的 `CompilerId*.c/cpp` 链进同一目标，出现 **multiple definition of `main`**）
  1. `mkdir -p build && cd build`
  2. `cmake ..` 或 `cmake -S .. -B .`
  3. `cmake --build .` 或 `make`
- 带 `configure` / `Makefile` + `setup/` 模板目录的项目（常见于某些科学计算或传统 Makefile 工程）：
  1. 先阅读 `README` / `INSTALL` / 上游文档，确认推荐的 `arch` 或配置模板名
  2. 若 `./configure` 不在白名单：用 `read_dir` 列出 `setup/`（或文档指明的模板目录），用 `read_file` 查看候选模板，再 `cp setup/<所选模板> Make.custom`（路径以实际目录为准）
  3. 在源码目录中执行 `make`，若 Makefile 要求 `arch=…`，按文档传入（例如 `make arch=<模板名>`）
  4. 若上述都失败，报告错误并说明已尝试的配置依据
- 普通 Make 项目：
  1. **在源码目录中**执行 `make`（必要时 `make -C SRC`）

**步骤 4: 处理编译错误（如果步骤 3 失败）**
如果编译失败，请分析错误类型并采取相应措施：

- **OpenMP 错误**（如 "'n' not specified in enclosing 'parallel'"）：
  * 原因：当前编译器版本与 OpenMP 配置不兼容
  * 解决：在 `setup/`（或文档说明的目录）中选更保守的模板复制为 `Make.custom` 后再 `make`

- **缺少依赖库**（如 "cannot find -lxxx"）：
  * 原因：系统缺少必要的开发库
  * 解决：尝试安装依赖或切换到不需要该库的配置

- **编译器版本不兼容**（如 "unrecognized command line option"）：
  * 原因：配置模板使用了当前编译器不支持的选项
  * 解决：切换到更基础的配置模板（以 `read_dir` + 文档为准）

- **配置错误**（如 "Please specify 'arch' variable"）：
  * 原因：Makefile 需要指定 arch 参数
  * 解决：按项目文档使用 `make arch=<模板名>` 或复制正确模板到 `Make.custom`

- **工作目录错误**（如 "没有指明目标并且找不到 makefile"）：
  * 原因：当前目录不是源码目录
  * 解决：使用 `-C` 指向经 `read_dir` 确认的源码目录，例如 `make -C SRC`

**步骤 5: 验证构建结果**
- 使用 `read_dir` 或 `run_command ls` 检查是否生成了可执行文件
- 如果构建成功，报告生成的可执行文件路径

**重要约束**：
- 如果步骤 2 发现编译器不存在，直接报告失败，不要继续尝试构建
- 如果命令返回"不在白名单中"，**不要**用其他 shell（bash/sh）重复尝试同一命令，这不会成功
- 最多尝试 3 种不同的构建方式，如果都失败则报告错误
- 对于 configure 项目，优先使用 setup/ 目录中的配置模板，而不是反复尝试 ./configure
- **编译失败后必须尝试其他配置**，不要重复尝试相同的配置"#;

pub(super) const TEST: &str = r#"这是一个测试/验证任务，请按以下步骤执行：

**步骤 0: 确定工作目录**
- 确认项目根目录和测试文件位置
- 使用 `-C` 选项或完整路径执行命令

**步骤 1: 检测测试框架**
- 使用 `read_dir` 查看项目结构
- 根据以下特征识别测试框架：
  * `Cargo.toml` + `#[cfg(test)]` → Rust: `cargo test`
  * `pytest.ini` / `conftest.py` / `test_*.py` → Python: `pytest`
  * `package.json` + `jest/mocha/vitest` → Node.js: `npm test`
  * `pom.xml` / `build.gradle` + `src/test/` → Java: `mvn test` / `gradle test`
  * `Makefile` 中的 `test` 目标 → `make test`
  * `CMakeLists.txt` 中的 `enable_testing()` → `ctest`

**步骤 2: 执行测试**
- 运行检测到的测试命令
- 如果测试框架不明确，按优先级尝试：`make test` → `cargo test` → `pytest` → `npm test`
- 对于特定测试用例，使用过滤参数（如 `cargo test test_name`、`pytest test_file.py`）

**步骤 3: 分析测试结果**
- **全部通过**：报告通过的测试数量和耗时
- **部分失败**：分析失败原因（断言错误、超时、依赖缺失等）
- **编译错误**：参考编译任务的错误处理步骤
- **超时**：尝试增加超时时间或运行子集测试

**步骤 4: 处理测试失败**
- **断言失败**：查看错误消息定位问题代码
- **依赖缺失**：安装缺失的测试依赖
- **环境问题**：检查环境变量、配置文件
- **竞态条件**：尝试串行运行测试

**步骤 5: 生成报告**
- 总结测试结果（通过/失败/跳过数量）
- 列出失败测试的原因
- 如果有覆盖率数据，报告覆盖率

**重要约束**：
- 最多尝试 3 种不同的测试运行方式
- 不要反复运行同一命令期望不同结果
- 如果测试需要特定环境（如数据库），报告缺失而非反复尝试"#;

pub(super) const DEBUG: &str = r#"这是一个调试/修复任务，请按以下步骤执行：

**步骤 0: 确定工作目录和问题上下文**
- 确认项目根目录
- 理解错误描述或用户反馈

**步骤 1: 收集信息**
- 使用 `read_file` 查看相关源代码
- 使用 `read_dir` 确认项目结构
- 如果有错误日志，使用 `read_file` 读取日志文件
- 使用 `run_command` 运行命令复现问题（如 `make`、`cargo build`）

**步骤 2: 定位问题**
- 根据错误信息定位代码位置（文件名:行号）
- 分析错误类型：
  * **编译错误**：语法错误、类型不匹配、缺少导入
  * **运行时错误**：空指针、越界访问、权限问题
  * **逻辑错误**：算法错误、条件判断错误
  * **配置错误**：路径错误、环境变量缺失

**步骤 3: 实施修复**
- 使用 `search_replace` 修改代码（小范围修改）
- 使用 `create_file` 创建新文件（如需添加配置）
- 修复原则：
  * 最小化修改范围，只修复问题本身
  * 不要重构或优化无关代码
  * 保持代码风格一致

**步骤 4: 验证修复**
- 重新编译/运行确认错误已解决
- 如果有测试，运行相关测试确认修复正确
- 如果修复引入新错误，回滚并尝试其他方案

**步骤 5: 总结**
- 描述问题根因
- 说明修复方案
- 报告验证结果

**重要约束**：
- 不要猜测问题原因，必须基于错误信息定位
- 不要同时修改多个不相关的文件
- 最多尝试 3 种修复方案
- 如果无法确定问题根因，报告分析结果而非盲目修改"#;

pub(super) const DEPLOY: &str = r#"这是一个部署/打包任务，请按以下步骤执行：

**步骤 0: 确定工作目录**
- 确认项目根目录和构建输出位置

**步骤 1: 检查部署环境**
- 使用 `which` 检查必要工具（docker、kubectl、rsync 等）
- 使用 `read_file` 查看部署配置（Dockerfile、docker-compose.yml、deploy.sh 等）
- 检查目标环境连接性（如需远程部署）

**步骤 2: 构建部署产物**
- 确保项目已编译成功（参考编译任务指导）
- 执行打包命令：
  * Rust: `cargo build --release`
  * Node.js: `npm run build`
  * Docker: `docker build -t image_name .`
  * 通用: `make package` / `make dist`

**步骤 3: 执行部署**
- **本地安装**: `make install` / `cargo install --path .`
- **Docker 部署**: `docker run` / `docker-compose up`
- **文件复制**: `cp` / `rsync` 到目标目录
- 注意工作目录，使用完整路径

**步骤 4: 验证部署**
- 检查部署产物是否到位（`read_dir`、`ls`）
- 如果是服务，检查是否运行（`ps`、`curl health endpoint`）
- 验证版本号和配置

**步骤 5: 清理（可选）**
- 清理临时构建文件
- 报告部署结果

**重要约束**：
- 不要在生产环境执行破坏性操作
- 确认构建成功后再部署
- 如果部署失败，检查日志而非反复重试
- 最多尝试 3 种部署方式"#;

pub(super) const REVIEW: &str = r#"这是一个代码审查/分析任务，请按以下步骤执行：

**步骤 0: 确定工作目录和审查范围**
- 确认项目根目录
- 理解审查范围（全部代码或特定文件/目录）

**步骤 1: 了解项目结构（先收窄再读文件）**
- 优先用 **`list_tree`** / **`glob_files`** / **`search_in_files`**（必要时 **`codebase_semantic_search`**，须已有索引）定位相关路径；可选用 **`repo_overview_sweep`** 做一次只读聚合素描，而不是无序遍历
- 仅在路径已明确必要时再用 **`read_file`**：`max_lines` 保持较小默认值或分段读取（`start_line`/`end_line`），大文件避免一次性拉大窗口
- 只针对少量关键清单文件读取全文或较长片段（例如 **`Cargo.toml`、`package.json`**、入口 `README`）；其余源码按「搜索结果 → 点到为止的几段 `read_file`」推进

**步骤 2: 执行静态分析**
- 根据项目语言选择工具：
  * Rust: `cargo clippy`（如果有 `Cargo.toml`）
  * Python: `python3 -m flake8` / `python3 -m pylint`
  * JavaScript/TypeScript: `npm run lint`
  * C/C++: `cppcheck` / `clang-tidy`
  * Shell: `shellcheck`
- 注意：工具可能未安装，如果不可用则跳过

**步骤 3: 代码审查要点**
- 基于步骤 1～2 已锁定的文件，用 `read_file` **按需阅读局部**（配合检索命中行附近），关注：
  * 潜在的 bug（空指针、越界、资源泄漏）
  * 安全问题（硬编码密钥、SQL 注入、XSS）
  * 代码风格（命名、格式、注释）
  * 性能问题（不必要的克隆、N+1 查询）
  * 可维护性（过长函数、深度嵌套）

**步骤 4: 生成审查报告**
- 按严重程度分类：🔴 严重 / 🟡 警告 / 🔵 建议
- 每个问题包含：文件位置、问题描述、修复建议
- 总结代码质量评分

**重要约束**：
- 不要修改代码，只做分析
- 不要猜测代码逻辑，基于实际代码分析
- 如果缺少分析工具，使用手动代码审查
- **禁止**在未先用检索/列目录收窄范围的情况下，对大量源文件逐个完整 `read_file`；若审查范围很大，先列出拟深入阅读的短清单（例如不超过十余个路径）再展开"#;

pub(super) const DEPS: &str = r#"这是一个依赖管理任务，请按以下步骤执行：

**步骤 0: 确定工作目录**
- 确认项目根目录

**步骤 1: 检测依赖文件**
- 使用 `read_dir` 查看项目结构
- 识别依赖管理文件：
  * Rust: `Cargo.toml` / `Cargo.lock`
  * Python: `requirements.txt` / `Pipfile` / `pyproject.toml`
  * Node.js: `package.json` / `package-lock.json` / `yarn.lock`
  * Java: `pom.xml` / `build.gradle`
  * C/C++: `vcpkg.json` / `conanfile.txt`

**步骤 2: 检查依赖状态**
- 使用 `read_file` 查看依赖文件内容
- 检查依赖是否已安装：
  * Rust: `cargo check`（自动下载依赖）
  * Python: `python3 -c "import pkg"` 逐个检查
  * Node.js: `ls node_modules/`
- 分析是否有版本冲突或安全漏洞

**步骤 3: 安装/更新依赖**
- 根据项目类型执行安装命令：
  * Rust: `cargo build`（自动安装依赖）
  * Python: `pip install -r requirements.txt`
  * Node.js: `npm install`
  * Java: `mvn install` / `gradle build`
- 注意工作目录，在包含依赖文件的目录中执行

**步骤 4: 验证依赖**
- 重新运行依赖检查确认安装成功
- 如果有测试，运行测试确认兼容性
- 检查是否有版本冲突警告

**步骤 5: 处理依赖问题**
- **安装失败**：检查网络连接、权限问题
- **版本冲突**：查看错误信息，调整版本约束
- **安全漏洞**：报告漏洞详情和建议升级版本
- **缺少系统依赖**：报告需要安装的系统包

**重要约束**：
- 不要修改依赖文件（除非明确要求更新依赖）
- 安装命令可能需要网络，报告网络错误而非反复重试
- 最多尝试 3 种安装方式"#;

pub(super) const CHECK_TOOLS: &str = r#"这是一个检查编译工具的任务：

**目标**：确认系统已安装必要的编译工具（gcc、g++、make、cmake、mpi 等）

**执行步骤**：
1. 使用 `which` 命令检查核心工具是否存在：
   - `which gcc g++ make`（基础编译工具）
   - `which cmake`（CMake 构建工具）
   - `which mpicc mpicxx mpirun`（MPI 并行工具，如需要）
   - `which gfortran`（Fortran 编译器，如需要）

2. 如果上述工具都存在，**立即报告任务完成**，无需重复检查其他工具

3. 如果某个工具不存在，报告缺失的工具名称

**重要约束**：
- **不要重复执行相同的 which 命令** - 一旦检测到工具存在，就不要再次检查
- **检测完成即结束** - 确认所有必需工具存在后，立即输出"工具检查完成"并结束任务
- 最多执行 3-5 个 which 命令，不要陷入无限检查循环"#;

pub(super) const FILE_OPS: &str = r#"这是一个文件操作任务：

**步骤 1: 确认路径**
- 使用 `read_dir` 确认目标目录存在
- 如果要修改文件，先用 `read_file` 查看当前内容

**步骤 2: 执行操作**
- 创建文件：使用 `create_file` 工具
- 修改文件：使用 `search_replace` 工具
- 删除文件：使用 `delete_file` 工具

**步骤 3: 验证**
- 使用 `read_file` 确认操作结果

**重要**：禁止假设文件存在，必须先确认再操作。"#;

pub(super) fn is_build_task(d: &str) -> bool {
    d.contains("编译")
        || d.contains("构建")
        || d.contains("build")
        || d.contains("make")
        || d.contains("cmake")
}

pub(super) fn is_test_task(d: &str) -> bool {
    d.contains("测试")
        || d.contains("test")
        || d.contains("unittest")
        || d.contains("benchmark")
        || d.contains("验证")
}

pub(super) fn is_debug_task(d: &str) -> bool {
    d.contains("调试")
        || d.contains("debug")
        || d.contains("修复")
        || d.contains("fix")
        || d.contains("排错")
        || d.contains("排查")
}

pub(super) fn is_deploy_task(d: &str) -> bool {
    d.contains("部署")
        || d.contains("deploy")
        || d.contains("安装")
        || d.contains("install")
        || d.contains("发布")
        || d.contains("publish")
        || d.contains("打包")
        || d.contains("package")
}

pub(super) fn is_review_task(d: &str) -> bool {
    d.contains("审查")
        || d.contains("review")
        || d.contains("分析代码")
        || d.contains("静态分析")
        || d.contains("lint")
        || d.contains("代码质量")
}

pub(super) fn is_deps_task(d: &str) -> bool {
    d.contains("依赖")
        || d.contains("dependency")
        || d.contains("安装依赖")
        || d.contains("更新依赖")
        || d.contains("npm install")
        || d.contains("pip install")
        || d.contains("cargo update")
}

pub(super) fn is_check_tools_task(d: &str) -> bool {
    d.contains("检查")
        && (d.contains("编译")
            || d.contains("工具")
            || d.contains("gcc")
            || d.contains("make")
            || d.contains("mpi"))
}

pub(super) fn is_file_ops_task(d: &str) -> bool {
    d.contains("创建") || d.contains("修改") || d.contains("编辑") || d.contains("写入")
}
