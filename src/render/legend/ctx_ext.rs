use crate::render::{ctx::Ctx, feature::Feature};
use postgres::Row;

impl Ctx {
    pub fn legend_features(
        &self,
        layer_name: &str,
        mut cb: impl FnMut() -> Result<Vec<Row>, postgres::Error>,
    ) -> Result<Vec<Feature>, postgres::Error> {
        let Some(ref legend) = self.legend else {
            let span = tracy_client::span!("fetch_db");

            span.emit_text(layer_name);

            return Ok(cb()?.into_iter().map(|row| row.into()).collect());
        };

        let Some(legend) = legend.get(layer_name) else {
            return Ok(vec![]);
        };

        Ok(legend
            .iter()
            .map(|props| Feature::LegendData(props.clone()))
            .collect())
    }
}
