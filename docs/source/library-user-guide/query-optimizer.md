<!---
  Licensed to the Apache Software Foundation (ASF) under one
  or more contributor license agreements.  See the NOTICE file
  distributed with this work for additional information
  regarding copyright ownership.  The ASF licenses this file
  to you under the Apache License, Version 2.0 (the
  "License"); you may not use this file except in compliance
  with the License.  You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

  Unless required by applicable law or agreed to in writing,
  software distributed under the License is distributed on an
  "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
  KIND, either express or implied.  See the License for the
  specific language governing permissions and limitations
  under the License.
-->

# DataFusion Query Optimizer

[DataFusion][df] is an extensible query execution framework, written in Rust, that uses Apache Arrow as its in-memory
format.

DataFusion has modular design, allowing individual crates to be re-used in other projects.

This crate is a submodule of DataFusion that provides a query optimizer for logical plans, and
contains an extensive set of OptimizerRules that may rewrite the plan and/or its expressions so
they execute more quickly while still computing the same result.

## Running the Optimizer

The following code demonstrates the basic flow of creating the optimizer with a default set of optimization rules
and applying it to a logical plan to produce an optimized logical plan.

```rust

use std::sync::Arc;
use datafusion::logical_expr::{col, lit, LogicalPlan, LogicalPlanBuilder};
use datafusion::optimizer::{OptimizerRule, OptimizerContext, Optimizer};

// We need a logical plan as the starting point. There are many ways to build a logical plan:
//
// The `datafusion-expr` crate provides a LogicalPlanBuilder
// The `datafusion-sql` crate provides a SQL query planner that can create a LogicalPlan from SQL
// The `datafusion` crate provides a DataFrame API that can create a LogicalPlan

let initial_logical_plan = LogicalPlanBuilder::empty(false).build().unwrap();

// use builtin rules or customized rules
let rules: Vec<Arc<dyn OptimizerRule + Send + Sync>> = vec![];

let optimizer = Optimizer::with_rules(rules);

let config = OptimizerContext::new().with_max_passes(16);

let optimized_plan = optimizer.optimize(initial_logical_plan.clone(), &config, observer);

fn observer(plan: &LogicalPlan, rule: &dyn OptimizerRule) {
    println!(
        "After applying rule '{}':\n{}",
        rule.name(),
        plan.display_indent()
    )
}
```

## Writing Optimization Rules

Please refer to the
[optimizer_rule.rs](../../../datafusion-examples/examples/optimizer_rule.rs)
example to learn more about the general approach to writing optimizer rules and
then move onto studying the existing rules.

`OptimizerRule` transforms one ['LogicalPlan'] into another which
computes the same results, but in a potentially more efficient
way. If there are no suitable transformations for the input plan,
the optimizer can simply return it as is.

All rules must implement the `OptimizerRule` trait.

```rust
# use datafusion::common::tree_node::Transformed;
# use datafusion::common::Result;
# use datafusion::logical_expr::LogicalPlan;
# use datafusion::optimizer::{OptimizerConfig, OptimizerRule};
#

#[derive(Default, Debug)]
struct MyOptimizerRule {}

impl OptimizerRule for MyOptimizerRule {
    fn name(&self) -> &str {
        "my_optimizer_rule"
    }

    fn rewrite(
        &self,
        plan: LogicalPlan,
        _config: &dyn OptimizerConfig,
    ) -> Result<Transformed<LogicalPlan>> {
        unimplemented!()
    }
}
```

## Providing Custom Rules

The optimizer can be created with a custom set of rules.

```rust
# use std::sync::Arc;
# use datafusion::logical_expr::{col, lit, LogicalPlan, LogicalPlanBuilder};
# use datafusion::optimizer::{OptimizerRule, OptimizerConfig, OptimizerContext, Optimizer};
# use datafusion::common::tree_node::Transformed;
# use datafusion::common::Result;
#
# #[derive(Default, Debug)]
# struct MyOptimizerRule {}
#
# impl OptimizerRule for MyOptimizerRule {
#     fn name(&self) -> &str {
#         "my_optimizer_rule"
#     }
#
#     fn rewrite(
#         &self,
#         plan: LogicalPlan,
#         _config: &dyn OptimizerConfig,
#     ) -> Result<Transformed<LogicalPlan>> {
#         unimplemented!()
#     }
# }

let optimizer = Optimizer::with_rules(vec![
    Arc::new(MyOptimizerRule {})
]);
```

### General Guidelines

Rules typical walk the logical plan and walk the expression trees inside operators and selectively mutate
individual operators or expressions.

Sometimes there is an initial pass that visits the plan and builds state that is used in a second pass that performs
the actual optimization. This approach is used in projection push down and filter push down.

### Expression Naming

Every expression in DataFusion has a name, which is used as the column name. For example, in this example the output
contains a single column with the name `"COUNT(aggregate_test_100.c9)"`:

```text
> select count(c9) from aggregate_test_100;
+------------------------------+
| COUNT(aggregate_test_100.c9) |
+------------------------------+
| 100                          |
+------------------------------+
```

These names are used to refer to the columns in both subqueries as well as internally from one stage of the LogicalPlan
to another. For example:

```text
> select "COUNT(aggregate_test_100.c9)" + 1 from (select count(c9) from aggregate_test_100) as sq;
+--------------------------------------------+
| sq.COUNT(aggregate_test_100.c9) + Int64(1) |
+--------------------------------------------+
| 101                                        |
+--------------------------------------------+
```

### Implication

Because DataFusion identifies columns using a string name, it means it is critical that the names of expressions are
not changed by the optimizer when it rewrites expressions. This is typically accomplished by renaming a rewritten
expression by adding an alias.

Here is a simple example of such a rewrite. The expression `1 + 2` can be internally simplified to 3 but must still be
displayed the same as `1 + 2`:

```text
> select 1 + 2;
+---------------------+
| Int64(1) + Int64(2) |
+---------------------+
| 3                   |
+---------------------+
```

Looking at the `EXPLAIN` output we can see that the optimizer has effectively rewritten `1 + 2` into effectively
`3 as "1 + 2"`:

```text
> explain format indent select 1 + 2;
+---------------+-------------------------------------------------+
| plan_type     | plan                                            |
+---------------+-------------------------------------------------+
| logical_plan  | Projection: Int64(3) AS Int64(1) + Int64(2)     |
|               |   EmptyRelation                                 |
| physical_plan | ProjectionExec: expr=[3 as Int64(1) + Int64(2)] |
|               |   PlaceholderRowExec                            |
|               |                                                 |
+---------------+-------------------------------------------------+
```

If the expression name is not preserved, bugs such as [#3704](https://github.com/apache/datafusion/issues/3704)
and [#3555](https://github.com/apache/datafusion/issues/3555) occur where the expected columns can not be found.

### Building Expression Names

There are currently two ways to create a name for an expression in the logical plan.

```rust
# use datafusion::common::Result;
# struct Expr;

impl Expr {
    /// Returns the name of this expression as it should appear in a schema. This name
    /// will not include any CAST expressions.
    pub fn display_name(&self) -> Result<String> {
        Ok("display_name".to_string())
    }

    /// Returns a full and complete string representation of this expression.
    pub fn canonical_name(&self) -> String {
        "canonical_name".to_string()
    }
}
```

When comparing expressions to determine if they are equivalent, `canonical_name` should be used, and when creating a
name to be used in a schema, `display_name` should be used.

### Utilities

There are a number of [utility methods][util] provided that take care of some common tasks.

[util]: https://github.com/apache/datafusion/blob/main/datafusion/expr/src/utils.rs

### Recursively walk an expression tree

The [TreeNode API] provides a convenient way to recursively walk an expression or plan tree.

For example, to find all subquery references in a logical plan, the following code can be used:

```rust
# use datafusion::prelude::*;
# use datafusion::common::tree_node::{TreeNode, TreeNodeRecursion};
# use datafusion::common::Result;
// Return all subquery references in an expression
fn extract_subquery_filters(expression: &Expr) -> Result<Vec<&Expr>> {
    let mut extracted = vec![];
    expression.apply(|expr| {
            if let Expr::InSubquery(_) = expr {
                extracted.push(expr);
            }
            Ok(TreeNodeRecursion::Continue)
    })?;
    Ok(extracted)
}
```

Likewise you can use the [TreeNode API] to rewrite a `LogicalPlan` or `ExecutionPlan`

```rust
# use datafusion::prelude::*;
# use datafusion::logical_expr::{LogicalPlan, Join};
# use datafusion::common::tree_node::{TreeNode, TreeNodeRecursion};
# use datafusion::common::Result;
// Return all joins in a logical plan
fn find_joins(overall_plan: &LogicalPlan) -> Result<Vec<&Join>> {
    let mut extracted = vec![];
    overall_plan.apply(|plan| {
            if let LogicalPlan::Join(join) = plan {
                extracted.push(join);
            }
            Ok(TreeNodeRecursion::Continue)
    })?;
    Ok(extracted)
}
```

### Rewriting expressions

The [TreeNode API] also provides a convenient way to rewrite expressions and
plans as well. For example to rewrite all expressions like

```sql
col BETWEEN x AND y
```

into

```sql
col >= x AND col <= y
```

you can use the following code:

```rust
# use datafusion::prelude::*;
# use datafusion::logical_expr::{Between};
# use datafusion::logical_expr::expr_fn::*;
# use datafusion::common::tree_node::{Transformed, TreeNode, TreeNodeRecursion};
# use datafusion::common::Result;
// Recursively rewrite all BETWEEN expressions
// returns Transformed::yes if any changes were made
fn rewrite_between(expr: Expr) -> Result<Transformed<Expr>> {
    // transform_up does a bottom up rewrite
    expr.transform_up(|expr| {
        // only handle BETWEEN expressions
        let Expr::Between(Between {
                negated,
                expr,
                low,
                high,
        }) = expr else {
            return Ok(Transformed::no(expr))
        };
        let rewritten_expr = if negated {
            // don't rewrite NOT BETWEEN
            Expr::Between(Between::new(expr, negated, low, high))
        } else {
            // rewrite to (expr >= low) AND (expr <= high)
            expr.clone().gt_eq(*low).and(expr.lt_eq(*high))
        };
        Ok(Transformed::yes(rewritten_expr))
    })
}
```

### Writing Tests

There should be unit tests in the same file as the new rule that test the effect of the rule being applied to a plan
in isolation (without any other rule being applied).

There should also be a test in `integration-tests.rs` that tests the rule as part of the overall optimization process.

### Debugging

The `EXPLAIN VERBOSE` command can be used to show the effect of each optimization rule on a query.

In the following example, the `type_coercion` and `simplify_expressions` passes have simplified the plan so that it returns the constant `"3.2"` rather than doing a computation at execution time.

```text
> explain verbose select cast(1 + 2.2 as string) as foo;
+------------------------------------------------------------+---------------------------------------------------------------------------+
| plan_type                                                  | plan                                                                      |
+------------------------------------------------------------+---------------------------------------------------------------------------+
| initial_logical_plan                                       | Projection: CAST(Int64(1) + Float64(2.2) AS Utf8) AS foo                  |
|                                                            |   EmptyRelation                                                           |
| logical_plan after type_coercion                           | Projection: CAST(CAST(Int64(1) AS Float64) + Float64(2.2) AS Utf8) AS foo |
|                                                            |   EmptyRelation                                                           |
| logical_plan after simplify_expressions                    | Projection: Utf8("3.2") AS foo                                            |
|                                                            |   EmptyRelation                                                           |
| logical_plan after unwrap_cast_in_comparison               | SAME TEXT AS ABOVE                                                        |
| logical_plan after decorrelate_where_exists                | SAME TEXT AS ABOVE                                                        |
| logical_plan after decorrelate_where_in                    | SAME TEXT AS ABOVE                                                        |
| logical_plan after scalar_subquery_to_join                 | SAME TEXT AS ABOVE                                                        |
| logical_plan after subquery_filter_to_join                 | SAME TEXT AS ABOVE                                                        |
| logical_plan after simplify_expressions                    | SAME TEXT AS ABOVE                                                        |
| logical_plan after eliminate_filter                        | SAME TEXT AS ABOVE                                                        |
| logical_plan after reduce_cross_join                       | SAME TEXT AS ABOVE                                                        |
| logical_plan after common_sub_expression_eliminate         | SAME TEXT AS ABOVE                                                        |
| logical_plan after eliminate_limit                         | SAME TEXT AS ABOVE                                                        |
| logical_plan after projection_push_down                    | SAME TEXT AS ABOVE                                                        |
| logical_plan after rewrite_disjunctive_predicate           | SAME TEXT AS ABOVE                                                        |
| logical_plan after reduce_outer_join                       | SAME TEXT AS ABOVE                                                        |
| logical_plan after filter_push_down                        | SAME TEXT AS ABOVE                                                        |
| logical_plan after limit_push_down                         | SAME TEXT AS ABOVE                                                        |
| logical_plan after single_distinct_aggregation_to_group_by | SAME TEXT AS ABOVE                                                        |
| logical_plan                                               | Projection: Utf8("3.2") AS foo                                            |
|                                                            |   EmptyRelation                                                           |
| initial_physical_plan                                      | ProjectionExec: expr=[3.2 as foo]                                         |
|                                                            |   PlaceholderRowExec                                                      |
|                                                            |                                                                           |
| physical_plan after aggregate_statistics                   | SAME TEXT AS ABOVE                                                        |
| physical_plan after join_selection                         | SAME TEXT AS ABOVE                                                        |
| physical_plan after coalesce_batches                       | SAME TEXT AS ABOVE                                                        |
| physical_plan after repartition                            | SAME TEXT AS ABOVE                                                        |
| physical_plan after add_merge_exec                         | SAME TEXT AS ABOVE                                                        |
| physical_plan                                              | ProjectionExec: expr=[3.2 as foo]                                         |
|                                                            |   PlaceholderRowExec                                                      |
|                                                            |                                                                           |
+------------------------------------------------------------+---------------------------------------------------------------------------+
```

[df]: https://crates.io/crates/datafusion

## Thinking about Query Optimization

Query optimization in DataFusion uses a cost based model. The cost based model
relies on table and column level statistics to estimate selectivity; selectivity
estimates are an important piece in cost analysis for filters and projections
as they allow estimating the cost of joins and filters.

An important piece of building these estimates is _boundary analysis_ which uses
interval arithmetic to take an expression such as `a > 2500 AND a <= 5000` and
build an accurate selectivity estimate that can then be used to find more efficient
plans.

### `AnalysisContext` API

The `AnalysisContext` serves as a shared knowledge base during expression evaluation
and boundary analysis. Think of it as a dynamic repository that maintains information about:

1. Current known boundaries for columns and expressions
2. Statistics that have been gathered or inferred
3. A mutable state that can be updated as analysis progresses

What makes `AnalysisContext` particularly powerful is its ability to propagate information
through the expression tree. As each node in the expression tree is analyzed, it can both
read from and write to this shared context, allowing for sophisticated boundary analysis and inference.

### `ColumnStatistics` for Cardinality Estimation

Column statistics form the foundation of optimization decisions. Rather than just tracking
simple metrics, DataFusion's `ColumnStatistics` provides a rich set of information including:

- Null value counts
- Maximum and minimum values
- Value sums (for numeric columns)
- Distinct value counts

Each of these statistics is wrapped in a `Precision` type that indicates whether the value is
exact or estimated, allowing the optimizer to make informed decisions about the reliability
of its cardinality estimates.

### Boundary Analysis Flow

The boundary analysis process flows through several stages, with each stage building
upon the information gathered in previous stages. The `AnalysisContext` is continuously
updated as the analysis progresses through the expression tree.

#### Expression Boundary Analysis

When analyzing expressions, DataFusion runs boundary analysis using interval arithmetic.
Consider a simple predicate like age > 18 AND age <= 25. The analysis flows as follows:

1. Context Initialization

   - Begin with known column statistics
   - Set up initial boundaries based on column constraints
   - Initialize the shared analysis context

2. Expression Tree Walk

   - Analyze each node in the expression tree
   - Propagate boundary information upward
   - Allow child nodes to influence parent boundaries

3. Boundary Updates
   - Each expression can update the shared context
   - Changes flow through the entire expression tree
   - Final boundaries inform optimization decisions

### Working with the analysis API

The following example shows how you can run an analysis pass on a physical expression
to infer the selectivity of the expression and the space of possible values it can
take.

```rust
# use std::sync::Arc;
# use datafusion::prelude::*;
# use datafusion::physical_expr::{analyze, AnalysisContext, ExprBoundaries};
# use datafusion::arrow::datatypes::{DataType, Field, Schema, TimeUnit};
# use datafusion::common::stats::Precision;
#
# use datafusion::common::{ColumnStatistics, DFSchema};
# use datafusion::common::{ScalarValue, ToDFSchema};
# use datafusion::error::Result;
fn analyze_filter_example() -> Result<()> {
    // Create a schema with an 'age' column
    let age = Field::new("age", DataType::Int64, false);
    let schema = Arc::new(Schema::new(vec![age]));

    // Define column statistics
    let column_stats = ColumnStatistics {
        null_count: Precision::Exact(0),
        max_value: Precision::Exact(ScalarValue::Int64(Some(79))),
        min_value: Precision::Exact(ScalarValue::Int64(Some(14))),
        distinct_count: Precision::Absent,
        sum_value: Precision::Absent,
    };

    // Create expression: age > 18 AND age <= 25
    let expr = col("age")
        .gt(lit(18i64))
        .and(col("age").lt_eq(lit(25i64)));

    // Initialize analysis context
    let initial_boundaries = vec![ExprBoundaries::try_from_column(
        &schema, &column_stats, 0)?];
    let context = AnalysisContext::new(initial_boundaries);

    // Analyze expression
    let df_schema = DFSchema::try_from(schema)?;
    let physical_expr = SessionContext::new().create_physical_expr(expr, &df_schema)?;
    let analysis = analyze(&physical_expr, context, df_schema.as_ref())?;

    Ok(())
}
```
