---
tags: [syntax, code]
---
# Code Blocks

```rust
fn main() {
    println!("{}", "[[Dead End]] is text here, not a link");
}
```

```python
def greet(name: str) -> str:
    return f"hello {name}"  # #not-a-tag
```

```javascript
const link = "[[Headings Target]]";
export default async () => link;
```

```typescript
type Note = { path: string; links: string[] };
```

```json
{ "key": [1, 2, 3], "nested": { "ok": true } }
```

```yaml
key: value
list:
  - a
  - b
```

```bash
echo "$HOME" | tr a-z A-Z
```

```css
.callout[data-callout="custom"] { --callout-color: #7b6cd9; }
```

```html
<div class="note"><span>text</span></div>
```

```sql
SELECT path FROM notes WHERE tag = 'project' ORDER BY path;
```

```c
int main(void) { return 0; }
```

```cpp
#include <vector>
std::vector<int> v{1, 2, 3};
```

```go
func main() { println("hi") }
```

```diff
- removed line
+ added line
```

```toml
[package]
name = "fixture"
```

```markdown
# A heading inside a code block
- [ ] a task inside a code block
```

```nonsense-language
an unknown language falls back to plain text
```

```
no language at all
```

~~~
A tilde fence
~~~

````
A four-backtick fence holding three:
```
inner
```
````

    An indented code block, four spaces.

Inline `code`, and `` code with a ` backtick ``.
