//! A demo project so the first run has something to explore. The three papers are
//! invented examples, labelled as such everywhere they appear. The code folder has
//! drafts with messy names and a few saved versions, so history and lineage show up.

use crate::extract::build_card;
use crate::readers::{Section, Source};
use crate::store::State;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn sec(h: &str, t: &str) -> Section { Section { heading: h.into(), text: t.into() } }

fn papers() -> Vec<Source> {
    vec![
        Source {
            title: "Demo paper A (invented example): Boosted trees for card-transaction fraud".into(),
            authors: vec!["Example Author".into(), "Second Author".into()], year: Some(2023), kind: "demo".into(),
            link: "https://example.org/researchtrail-demo/paper-a".into(),
            sections: vec![
                sec("Abstract", "We study fraud detection on card transactions with gradient boosted trees. Our LightGBM model reaches a ROC-AUC of 0.93 and a PR-AUC of 0.61."),
                sec("1 Introduction", "Earlier work relied on logistic regression and a support vector machine."),
                sec("3.1 Data Preprocessing", "Missing values were imputed with the median of each column. Because fraud is rare, we applied SMOTE to the training folds only."),
                sec("3.2 Feature Engineering", "We built frequency encoding features for card and merchant identifiers. Aggregation features per card, such as the mean amount over the previous day, were added. Time-based features such as hour of day captured night-time spending."),
                sec("3.3 Model", "The final model is LightGBM with 500 trees."),
                sec("4 Experimental Setup", "Hyperparameters were tuned with Optuna for 100 trials. The learning rate was 0.05 and training used early stopping after 50 rounds."),
                sec("5 Results", "With stratified 5-fold cross-validation the model achieves a ROC-AUC of 0.93. Precision of 0.72 and recall of 0.58 were reported at the chosen threshold."),
            ],
            ..Default::default()
        },
        Source {
            title: "Demo paper B (invented example): Graph features for fraud rings".into(),
            authors: vec!["Another Example".into()], year: Some(2024), kind: "demo".into(),
            link: "https://example.org/researchtrail-demo/paper-b".into(),
            sections: vec![
                sec("Abstract", "Fraudsters often act in rings that share devices and addresses. We add graph features and a graph neural network to capture these links."),
                sec("2 Data", "We use the Elliptic dataset and a private transaction log. A time-based split keeps the last two months for testing."),
                sec("3 Preprocessing", "All numeric features were standardized. The majority class was reduced with random undersampling."),
                sec("4 Features", "For each account we compute graph features such as node degree and PageRank in the device-sharing graph. Merchant category is handled with target encoding."),
                sec("5 Method", "Our main model is a GraphSAGE graph neural network, compared with a random forest baseline."),
                sec("6 Training", "We train with the Adam optimizer, a learning rate of 0.001 and dropout of 0.3 for 200 epochs."),
                sec("7 Evaluation", "On the held-out months the GNN reaches an F1 score of 0.71 against 0.63 for the random forest."),
            ],
            ..Default::default()
        },
        Source {
            title: "Demo paper C (invented example): Interpretable scoring with calibrated models".into(),
            authors: vec!["Third Example".into(), "Coauthor".into()], year: Some(2022), kind: "demo".into(),
            link: "https://example.org/researchtrail-demo/paper-c".into(),
            sections: vec![
                sec("Abstract", "Regulated lenders need models they can explain. We compare logistic regression with XGBoost and explain both with SHAP."),
                sec("3 Data Preparation", "Rows with missing values were removed, then features were standardized. Categorical fields used one-hot encoding."),
                sec("4 Models", "We train logistic regression with class weights and an XGBoost classifier."),
                sec("5 Evaluation", "Models are compared with ROC-AUC and the Brier score using stratified 10-fold cross-validation. SHAP values explain individual decisions, and calibration curves check probability quality."),
            ],
            ..Default::default()
        },
    ]
}

const NB_DRAFT: &str = r##"{"cells":[
 {"cell_type":"markdown","metadata":{},"source":["# Feature engineering (first try)"]},
 {"cell_type":"code","execution_count":1,"metadata":{},"outputs":[],"source":["import pandas as pd\n","df = pd.read_csv('transactions.csv')\n","df = df.dropna()\n"]},
 {"cell_type":"code","execution_count":2,"metadata":{},"outputs":[],"source":["df['card_freq'] = df['card_id'].map(df['card_id'].value_counts())\n"]}
],"metadata":{},"nbformat":4,"nbformat_minor":5}"##;

const NB_FINAL: &str = r##"{"cells":[
 {"cell_type":"markdown","metadata":{},"source":["# Feature engineering"]},
 {"cell_type":"code","execution_count":1,"metadata":{},"outputs":[],"source":["import pandas as pd\n","df = pd.read_csv('transactions.csv')\n","df = df.dropna()\n"]},
 {"cell_type":"code","execution_count":2,"metadata":{},"outputs":[],"source":["df['card_freq'] = df['card_id'].map(df['card_id'].value_counts())\n","df['merchant_freq'] = df['merchant'].map(df['merchant'].value_counts())\n"]},
 {"cell_type":"code","execution_count":3,"metadata":{},"outputs":[],"source":["df['card_mean_amt'] = df.groupby('card_id')['amount'].transform('mean')\n","df['hour'] = pd.to_datetime(df['time']).dt.hour\n"]},
 {"cell_type":"code","execution_count":null,"metadata":{},"outputs":[],"source":["# tried this, didn't keep it\n","from sklearn.decomposition import PCA\n","X_small = PCA(10).fit_transform(X)\n"]}
],"metadata":{},"nbformat":4,"nbformat_minor":5}"##;

const TRAIN_1: &str = "import lightgbm as lgb\nfrom sklearn.model_selection import train_test_split\nfrom sklearn.metrics import roc_auc_score\n\nlearning_rate = 0.1\n\ndef train(X, y):\n    X_tr, X_te, y_tr, y_te = train_test_split(X, y, test_size=0.2)\n    model = lgb.LGBMClassifier(learning_rate=learning_rate, n_estimators=200)\n    model.fit(X_tr, y_tr)\n    return roc_auc_score(y_te, model.predict_proba(X_te)[:, 1])\n";
const TRAIN_2: &str = "import lightgbm as lgb\nfrom imblearn.over_sampling import SMOTE\nfrom sklearn.model_selection import StratifiedKFold\nfrom sklearn.metrics import roc_auc_score, average_precision_score\n\nlearning_rate = 0.05\n\ndef train(X, y):\n    scores = []\n    for tr, te in StratifiedKFold(n_splits=5, shuffle=True).split(X, y):\n        X_tr, y_tr = SMOTE().fit_resample(X[tr], y[tr])\n        model = lgb.LGBMClassifier(learning_rate=learning_rate, n_estimators=500)\n        model.fit(X_tr, y_tr, callbacks=[lgb.early_stopping(50)])\n        p = model.predict_proba(X[te])[:, 1]\n        scores.append((roc_auc_score(y[te], p), average_precision_score(y[te], p)))\n    return scores\n";
const TRAIN_FINAL: &str = "import lightgbm as lgb\nfrom imblearn.over_sampling import SMOTE\nfrom sklearn.model_selection import StratifiedKFold\nfrom sklearn.metrics import roc_auc_score, average_precision_score\nfrom sklearn.preprocessing import QuantileTransformer\n\nlearning_rate = 0.03\n\ndef train(X, y):\n    scores = []\n    X = QuantileTransformer().fit_transform(X)\n    for tr, te in StratifiedKFold(n_splits=5, shuffle=True).split(X, y):\n        X_tr, y_tr = SMOTE().fit_resample(X[tr], y[tr])\n        model = lgb.LGBMClassifier(learning_rate=learning_rate, n_estimators=800)\n        model.fit(X_tr, y_tr, callbacks=[lgb.early_stopping(50)])\n        p = model.predict_proba(X[te])[:, 1]\n        scores.append((roc_auc_score(y[te], p), average_precision_score(y[te], p)))\n    return scores\n";

fn write_aged(path: &Path, body: &str, hours_ago: u64) {
    if let Some(p) = path.parent() { let _ = fs::create_dir_all(p); }
    let _ = fs::write(path, body);
    if let Ok(f) = fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now() - Duration::from_secs(hours_ago * 3600));
    }
}

pub fn demo_dir(home: &Path) -> PathBuf { home.join("ResearchTrail demo") }

pub fn create(home: &Path) -> Result<PathBuf, String> {
    let dir = demo_dir(home);
    if dir.join(".trail").exists() { return Ok(dir); }
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create the demo folder: {}", e))?;
    let _ = fs::write(dir.join("README.txt"), "This is the ResearchTrail demo project. The three papers are invented examples; the code shows drafts with messy names.\nDelete this folder whenever you like.\n");
    write_aged(&dir.join("notebooks/features_draft.ipynb"), NB_DRAFT, 72);
    write_aged(&dir.join("train.py"), TRAIN_1, 70);
    let mut st = State::open(&dir)?;
    write_aged(&dir.join("notebooks/features_final.ipynb"), NB_FINAL, 30);
    write_aged(&dir.join("train.py"), TRAIN_2, 26);
    st.scan_code(false);
    write_aged(&dir.join("train_final.py"), TRAIN_FINAL, 2);
    st.scan_code(false);
    let mut ids = BTreeSet::new();
    for (i, src) in papers().into_iter().enumerate() {
        let mut card = build_card(&src, &st.catalog, &ids);
        card.added = crate::util::now().saturating_sub((60 - i as u64 * 20) * 3600);
        card.reader_notes.push("Invented example paper for the demo. Real papers come in when you paste a link.".into());
        ids.insert(card.id.clone());
        st.save_card(&card)?;
    }
    Ok(dir)
}
